// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! X11 / XWayland graphics tablet input via XInput 2.2 (`x11rb::protocol::xinput`).
//!
//! Opens a dedicated X11 connection for the active window, discovers slave
//! stylus and eraser devices that expose an absolute pressure valuator, and
//! translates slave `XI_Motion`, `XI_ButtonPress`, `XI_ButtonRelease`, and
//! `XI_Leave` events into [`PointerEvent`]s in client-area physical pixels.
//! Master pointer events carrying a non-pen `sourceid` immediately clear pen
//! proximity so switching back to a physical mouse never stalls.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use ugu_platform_types::pointer::{PointerEvent, PointerId, PointerKind, PointerSample};
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{
    ConnectionExt as _, Device, DeviceClass, DeviceClassData, DeviceId, DeviceType, EventMask,
    Fp3232, XIDeviceInfo, XIEventMask,
};
use x11rb::protocol::xproto::{Atom, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::clock;
use super::wayland_tablet::{PEN_POINTER_BASE, PenQueue, pen_buttons};

pub(crate) struct X11TabletHandle {
    stop_fd: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl X11TabletHandle {
    pub(crate) fn spawn(window: Window, queue: Arc<PenQueue>) -> Option<Self> {
        // SAFETY: `EFD_CLOEXEC | EFD_NONBLOCK` creates a fresh kernel eventfd.
        let raw_efd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if raw_efd < 0 {
            return None;
        }
        // SAFETY: `raw_efd >= 0` is a newly created descriptor.
        let stop_fd = Arc::new(unsafe { OwnedFd::from_raw_fd(raw_efd) });
        let worker_stop = stop_fd.clone();

        let thread = std::thread::Builder::new()
            .name("x11-tablet".to_owned())
            .spawn(move || run_x11_tablet(window, queue, worker_stop))
            .ok()?;

        Some(Self {
            stop_fd,
            thread: Some(thread),
        })
    }

    pub(crate) fn shutdown(&mut self) {
        let one = 1u64.to_ne_bytes();
        // SAFETY: `stop_fd` is a valid open non-blocking eventfd.
        unsafe {
            let _ = libc::write(self.stop_fd.as_raw_fd(), one.as_ptr().cast(), one.len());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TabletAtoms {
    pub(crate) abs_pressure: Atom,
    pub(crate) abs_tilt_x: Atom,
    pub(crate) abs_tilt_y: Atom,
    pub(crate) abs_wheel: Atom,
    pub(crate) abs_rz: Atom,
    pub(crate) btn_stylus: Atom,
    pub(crate) btn_stylus2: Atom,
}

impl TabletAtoms {
    fn intern_all(conn: &RustConnection) -> Option<Self> {
        let c_pressure = conn.intern_atom(false, b"Abs Pressure").ok()?;
        let c_tilt_x = conn.intern_atom(false, b"Abs Tilt X").ok()?;
        let c_tilt_y = conn.intern_atom(false, b"Abs Tilt Y").ok()?;
        let c_wheel = conn.intern_atom(false, b"Abs Wheel").ok()?;
        let c_rz = conn.intern_atom(false, b"Abs RZ").ok()?;
        let c_stylus = conn.intern_atom(false, b"Button Stylus").ok()?;
        let c_stylus2 = conn.intern_atom(false, b"Button Stylus2").ok()?;

        Some(Self {
            abs_pressure: c_pressure.reply().ok()?.atom,
            abs_tilt_x: c_tilt_x.reply().ok()?.atom,
            abs_tilt_y: c_tilt_y.reply().ok()?.atom,
            abs_wheel: c_wheel.reply().ok()?.atom,
            abs_rz: c_rz.reply().ok()?.atom,
            btn_stylus: c_stylus.reply().ok()?.atom,
            btn_stylus2: c_stylus2.reply().ok()?.atom,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisSpec {
    pub(crate) number: u16,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) last: f64,
}

impl AxisSpec {
    fn normalized_unit(self) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        (((self.last - self.min) / span) as f32).clamp(0.0, 1.0)
    }

    fn tilt_degrees(self) -> f32 {
        if self.min >= -90.0 && self.max <= 90.0 && self.max > self.min {
            (self.last as f32).clamp(-90.0, 90.0)
        } else if self.max > self.min {
            let norm = (self.last - self.min) / (self.max - self.min);
            ((norm * 180.0 - 90.0) as f32).clamp(-90.0, 90.0)
        } else {
            0.0
        }
    }

    fn twist_degrees(self) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        let deg = ((self.last - self.min) / span) * 360.0;
        (deg as f32).rem_euclid(360.0)
    }
}

/// Per-device XInput2 state for a discovered stylus or eraser slave device.
#[derive(Clone, Debug)]
pub(crate) struct X11PenDevice {
    pub(crate) pointer: PointerId,
    pub(crate) is_eraser: bool,
    pub(crate) barrel_button: u32,
    pub(crate) stylus2_button: u32,
    pub(crate) pressure: Option<AxisSpec>,
    pub(crate) tilt_x: Option<AxisSpec>,
    pub(crate) tilt_y: Option<AxisSpec>,
    pub(crate) twist: Option<AxisSpec>,
    pub(crate) tip_down: bool,
    pub(crate) barrel_held: bool,
    pub(crate) stylus2_held: bool,
    pub(crate) frame_counter: u32,
}

impl X11PenDevice {
    pub(crate) fn from_device_info(info: &XIDeviceInfo, atoms: &TabletAtoms) -> Option<Self> {
        if !info.enabled
            || (info.type_ != DeviceType::SLAVE_POINTER && info.type_ != DeviceType::FLOATING_SLAVE)
        {
            return None;
        }
        let name_lower = String::from_utf8_lossy(&info.name).to_ascii_lowercase();
        if is_excluded_device_name(&name_lower) {
            return None;
        }
        if info
            .classes
            .iter()
            .any(|cls| matches!(cls.data, DeviceClassData::Touch(_)))
        {
            return None;
        }

        let is_named_pen = name_lower.contains("stylus") || name_lower.contains("eraser");
        let is_eraser = name_lower.contains("eraser");

        let mut pressure = None;
        let mut tilt_x = None;
        let mut tilt_y = None;
        let mut twist = None;
        let mut barrel_button = None;
        let mut stylus2_button = None;

        for cls in &info.classes {
            match &cls.data {
                DeviceClassData::Valuator(v) => {
                    let spec = AxisSpec {
                        number: v.number,
                        min: fp3232_to_f64(v.min),
                        max: fp3232_to_f64(v.max),
                        last: fp3232_to_f64(v.value),
                    };
                    if atoms.abs_pressure != 0 && v.label == atoms.abs_pressure {
                        pressure = Some(spec);
                    } else if atoms.abs_tilt_x != 0 && v.label == atoms.abs_tilt_x {
                        tilt_x = Some(spec);
                    } else if atoms.abs_tilt_y != 0 && v.label == atoms.abs_tilt_y {
                        tilt_y = Some(spec);
                    } else if (atoms.abs_wheel != 0 && v.label == atoms.abs_wheel)
                        || (atoms.abs_rz != 0 && v.label == atoms.abs_rz)
                    {
                        twist = Some(spec);
                    } else if v.label == 0 && is_named_pen && v.number == 2 && spec.max > spec.min {
                        pressure.get_or_insert(spec);
                    }
                }
                DeviceClassData::Button(btn) => {
                    for (idx, &label) in btn.labels.iter().enumerate() {
                        let number = (idx + 1) as u32;
                        if atoms.btn_stylus != 0 && label == atoms.btn_stylus {
                            barrel_button = Some(number);
                        } else if atoms.btn_stylus2 != 0 && label == atoms.btn_stylus2 {
                            stylus2_button = Some(number);
                        }
                    }
                }
                _ => {}
            }
        }

        // Require an absolute pressure axis (or an explicitly named stylus/eraser).
        if pressure.is_none() && !is_named_pen {
            return None;
        }

        let barrel = barrel_button.unwrap_or(if stylus2_button == Some(3) { 2 } else { 3 });
        let stylus2 = stylus2_button.unwrap_or(if barrel == 2 { 3 } else { 2 });

        Some(Self {
            pointer: PointerId(PEN_POINTER_BASE.wrapping_add(u32::from(info.deviceid))),
            is_eraser,
            barrel_button: barrel,
            stylus2_button: stylus2,
            pressure,
            tilt_x,
            tilt_y,
            twist,
            tip_down: false,
            barrel_held: false,
            stylus2_held: false,
            frame_counter: 0,
        })
    }

    pub(crate) fn update_classes(&mut self, classes: &[DeviceClass], atoms: &TabletAtoms) {
        for cls in classes {
            if let DeviceClassData::Valuator(v) = &cls.data {
                let spec = AxisSpec {
                    number: v.number,
                    min: fp3232_to_f64(v.min),
                    max: fp3232_to_f64(v.max),
                    last: fp3232_to_f64(v.value),
                };
                if (atoms.abs_pressure != 0 && v.label == atoms.abs_pressure)
                    || self.pressure.is_some_and(|p| p.number == v.number)
                {
                    self.pressure = Some(spec);
                } else if (atoms.abs_tilt_x != 0 && v.label == atoms.abs_tilt_x)
                    || self.tilt_x.is_some_and(|t| t.number == v.number)
                {
                    self.tilt_x = Some(spec);
                } else if (atoms.abs_tilt_y != 0 && v.label == atoms.abs_tilt_y)
                    || self.tilt_y.is_some_and(|t| t.number == v.number)
                {
                    self.tilt_y = Some(spec);
                } else if (atoms.abs_wheel != 0 && v.label == atoms.abs_wheel)
                    || (atoms.abs_rz != 0 && v.label == atoms.abs_rz)
                    || self.twist.is_some_and(|t| t.number == v.number)
                {
                    self.twist = Some(spec);
                }
            }
        }
    }

    pub(crate) fn apply_button(&mut self, detail: u32, pressed: bool) {
        if detail == 1 {
            self.tip_down = pressed;
        } else if detail == self.barrel_button {
            self.barrel_held = pressed;
        } else if detail == self.stylus2_button {
            self.stylus2_held = pressed;
        }
    }

    pub(crate) fn apply_valuators(&mut self, valuator_mask: &[u32], axisvalues: &[Fp3232]) {
        let mut iter = axisvalues.iter().copied();
        for (word_idx, &word) in valuator_mask.iter().enumerate() {
            if word == 0 {
                continue;
            }
            for bit in 0..32u16 {
                if (word & (1u32 << bit)) == 0 {
                    continue;
                }
                let Some(raw) = iter.next() else {
                    return;
                };
                let axis_num = (word_idx as u16) * 32 + bit;
                let val = fp3232_to_f64(raw);
                for slot in [
                    &mut self.pressure,
                    &mut self.tilt_x,
                    &mut self.tilt_y,
                    &mut self.twist,
                ]
                .into_iter()
                .flatten()
                {
                    if slot.number == axis_num {
                        slot.last = val;
                    }
                }
            }
        }
    }

    pub(crate) fn build_sample(
        &mut self,
        event_x: i32,
        event_y: i32,
        time: ugu_platform_types::clock::Ticks,
    ) -> PointerSample {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        let (buttons, in_contact) = pen_buttons(
            self.tip_down,
            self.barrel_held,
            self.stylus2_held,
            self.is_eraser,
        );
        let pressure = self.pressure.map(|spec| {
            if self.tip_down {
                spec.normalized_unit()
            } else {
                0.0
            }
        });
        let tilt = match (self.tilt_x, self.tilt_y) {
            (Some(tx), Some(ty)) => Some([tx.tilt_degrees(), ty.tilt_degrees()]),
            (Some(tx), None) => Some([tx.tilt_degrees(), 0.0]),
            (None, Some(ty)) => Some([0.0, ty.tilt_degrees()]),
            (None, None) => None,
        };
        let twist = self.twist.map(AxisSpec::twist_degrees);

        PointerSample {
            position: [fp1616_to_f64(event_x), fp1616_to_f64(event_y)],
            pressure,
            tilt,
            twist,
            buttons,
            in_contact,
            inverted: self.is_eraser,
            time,
            frame_id: self.frame_counter,
        }
    }

    pub(crate) fn any_held(&self) -> bool {
        self.tip_down || self.barrel_held || self.stylus2_held
    }
}

fn is_excluded_device_name(name_lower: &str) -> bool {
    name_lower.contains("touchpad")
        || name_lower.contains("trackpad")
        || name_lower.contains("finger")
        || name_lower.contains("cursor")
        || name_lower.contains("mouse")
        || name_lower == "pad"
        || name_lower.contains(" pad")
        || name_lower.contains("-pad")
}

pub(crate) fn fp1616_to_f64(value: i32) -> f64 {
    f64::from(value) / 65_536.0
}

pub(crate) fn fp3232_to_f64(value: Fp3232) -> f64 {
    f64::from(value.integral) + f64::from(value.frac) / 4_294_967_296.0
}

fn xi_mask(flags: &[XIEventMask]) -> XIEventMask {
    let bits = flags.iter().fold(0u32, |acc, &flag| acc | u32::from(flag));
    XIEventMask::from(bits)
}

fn refresh_pen_devices(
    conn: &RustConnection,
    window: Window,
    atoms: &TabletAtoms,
    devices: &mut HashMap<DeviceId, X11PenDevice>,
) {
    let Ok(cookie) = conn.xinput_xi_query_device(Device::ALL) else {
        return;
    };
    let Ok(reply) = cookie.reply() else {
        return;
    };

    let mut updated = HashMap::new();
    let mut masks = Vec::new();
    let slave_mask = xi_mask(&[
        XIEventMask::MOTION,
        XIEventMask::BUTTON_PRESS,
        XIEventMask::BUTTON_RELEASE,
        XIEventMask::LEAVE,
        XIEventMask::DEVICE_CHANGED,
    ]);

    for info in &reply.infos {
        if let Some(mut discovered) = X11PenDevice::from_device_info(info, atoms) {
            if let Some(prev) = devices.get(&info.deviceid) {
                discovered.tip_down = prev.tip_down;
                discovered.barrel_held = prev.barrel_held;
                discovered.stylus2_held = prev.stylus2_held;
                discovered.frame_counter = prev.frame_counter;
            }
            masks.push(EventMask {
                deviceid: info.deviceid,
                mask: vec![slave_mask],
            });
            updated.insert(info.deviceid, discovered);
        }
    }

    // Watch master pointer motion/buttons so a physical mouse (`sourceid` not
    // in `updated`) immediately releases pen proximity suppression.
    masks.push(EventMask {
        deviceid: DeviceId::from(Device::ALL_MASTER),
        mask: vec![xi_mask(&[XIEventMask::MOTION, XIEventMask::BUTTON_PRESS])],
    });

    let _ = conn.xinput_xi_select_events(window, &masks);
    let _ = conn.flush();
    *devices = updated;
}

fn run_x11_tablet(window: Window, queue: Arc<PenQueue>, stop_fd: Arc<OwnedFd>) {
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return;
    };
    let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
        return;
    };
    let Ok(ver_cookie) = conn.xinput_xi_query_version(2, 2) else {
        return;
    };
    let Ok(ver) = ver_cookie.reply() else {
        return;
    };
    if ver.major_version < 2 {
        return;
    }
    let Some(atoms) = TabletAtoms::intern_all(&conn) else {
        return;
    };

    // `XI_HierarchyChanged` may only be selected on the root window in XI2.
    let _ = conn.xinput_xi_select_events(
        root,
        &[EventMask {
            deviceid: DeviceId::from(Device::ALL),
            mask: vec![XIEventMask::HIERARCHY],
        }],
    );

    let mut devices: HashMap<DeviceId, X11PenDevice> = HashMap::new();
    refresh_pen_devices(&conn, window, &atoms, &mut devices);

    let x11_fd = conn.stream().as_raw_fd();
    loop {
        while let Ok(Some(event)) = conn.poll_for_event() {
            handle_x11_event(event, window, &conn, &atoms, &mut devices, &queue);
        }

        let mut fds = [
            libc::pollfd {
                fd: x11_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop_fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` is a live 2-element stack array of open descriptors.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ready < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[1].revents & libc::POLLIN != 0 {
            break;
        }
        if fds[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            break;
        }
    }
}

fn handle_x11_event(
    event: Event,
    window: Window,
    conn: &RustConnection,
    atoms: &TabletAtoms,
    devices: &mut HashMap<DeviceId, X11PenDevice>,
    queue: &PenQueue,
) {
    match event {
        Event::XinputHierarchy(_) => {
            refresh_pen_devices(conn, window, atoms, devices);
        }
        Event::XinputDeviceChanged(ev) => {
            if let Some(dev) = devices.get_mut(&ev.deviceid) {
                dev.update_classes(&ev.classes, atoms);
            } else if let Some(dev) = devices.get_mut(&ev.sourceid) {
                dev.update_classes(&ev.classes, atoms);
            }
        }
        Event::XinputButtonPress(ev) if ev.event == window => {
            if let Some(dev) = devices.get_mut(&ev.deviceid) {
                dev.apply_button(ev.detail, true);
                dev.apply_valuators(&ev.valuator_mask, &ev.axisvalues);
                let time = clock::from_wrapped_millis(ev.time, clock::now());
                let sample = dev.build_sample(ev.event_x, ev.event_y, time);
                queue.push_x11(
                    dev.pointer,
                    PointerEvent::Samples {
                        pointer: dev.pointer,
                        kind: PointerKind::Pen,
                        samples: vec![sample],
                    },
                    dev.any_held(),
                    Instant::now(),
                );
            } else if !devices.contains_key(&ev.sourceid) {
                queue.clear_x11_from_non_pen();
            }
        }
        Event::XinputButtonRelease(ev) if ev.event == window => {
            if let Some(dev) = devices.get_mut(&ev.deviceid) {
                dev.apply_button(ev.detail, false);
                dev.apply_valuators(&ev.valuator_mask, &ev.axisvalues);
                let time = clock::from_wrapped_millis(ev.time, clock::now());
                let sample = dev.build_sample(ev.event_x, ev.event_y, time);
                queue.push_x11(
                    dev.pointer,
                    PointerEvent::Samples {
                        pointer: dev.pointer,
                        kind: PointerKind::Pen,
                        samples: vec![sample],
                    },
                    dev.any_held(),
                    Instant::now(),
                );
            }
        }
        Event::XinputMotion(ev) if ev.event == window => {
            if let Some(dev) = devices.get_mut(&ev.deviceid) {
                dev.apply_valuators(&ev.valuator_mask, &ev.axisvalues);
                let time = clock::from_wrapped_millis(ev.time, clock::now());
                let sample = dev.build_sample(ev.event_x, ev.event_y, time);
                queue.push_x11(
                    dev.pointer,
                    PointerEvent::Samples {
                        pointer: dev.pointer,
                        kind: PointerKind::Pen,
                        samples: vec![sample],
                    },
                    dev.any_held(),
                    Instant::now(),
                );
            } else if !devices.contains_key(&ev.sourceid) {
                queue.clear_x11_from_non_pen();
            }
        }
        Event::XinputLeave(ev) if ev.event == window => {
            if let Some(dev) = devices.get_mut(&ev.deviceid)
                && !dev.any_held()
            {
                queue.leave_x11(dev.pointer);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use ugu_platform_types::clock::Ticks;
    use ugu_platform_types::pointer::Buttons;
    use x11rb::protocol::xinput::{DeviceClassDataButton, DeviceClassDataValuator, ValuatorMode};

    use super::*;

    fn fp32(integral: i32, frac: u32) -> Fp3232 {
        Fp3232 { integral, frac }
    }

    fn valuator_class(
        sourceid: u16,
        number: u16,
        label: Atom,
        min: i32,
        max: i32,
        value: i32,
    ) -> DeviceClass {
        DeviceClass {
            len: 11,
            sourceid,
            data: DeviceClassData::Valuator(DeviceClassDataValuator {
                number,
                label,
                min: fp32(min, 0),
                max: fp32(max, 0),
                value: fp32(value, 0),
                resolution: 1,
                mode: ValuatorMode::ABSOLUTE,
            }),
        }
    }

    fn sample_atoms() -> TabletAtoms {
        TabletAtoms {
            abs_pressure: 10,
            abs_tilt_x: 11,
            abs_tilt_y: 12,
            abs_wheel: 13,
            abs_rz: 14,
            btn_stylus: 20,
            btn_stylus2: 21,
        }
    }

    #[test]
    fn fixed_point_decoders_preserve_negative_fractions() {
        assert_eq!(fp1616_to_f64(100 * 65_536 + 32_768), 100.5);
        assert_eq!(fp3232_to_f64(fp32(-16, 0x8000_0000)), -15.5);
    }

    #[test]
    fn xwayland_stylus_and_eraser_decode_axes_and_labeled_buttons() {
        let atoms = sample_atoms();
        // XWayland maps `Button Stylus` onto button 2 and `Button Stylus2` onto button 3.
        let classes = vec![
            DeviceClass {
                len: 4,
                sourceid: 9,
                data: DeviceClassData::Button(DeviceClassDataButton {
                    state: vec![0],
                    labels: vec![0, atoms.btn_stylus, atoms.btn_stylus2],
                }),
            },
            valuator_class(9, 2, atoms.abs_pressure, 0, 4096, 0),
            valuator_class(9, 3, atoms.abs_tilt_x, -64, 63, 0),
            valuator_class(9, 4, atoms.abs_tilt_y, -64, 63, 0),
        ];

        let eraser_info = XIDeviceInfo {
            deviceid: 9,
            type_: DeviceType::SLAVE_POINTER,
            attachment: 2,
            enabled: true,
            name: b"xwayland-tablet eraser:15".to_vec(),
            classes: classes.clone(),
        };
        let mut dev = X11PenDevice::from_device_info(&eraser_info, &atoms).unwrap();
        assert!(dev.is_eraser);
        assert_eq!(dev.barrel_button, 2);
        assert_eq!(dev.stylus2_button, 3);

        // Press barrel (button 2) + tip (button 1) with pressure = 2048 and tilt = (-12.5, 25.0).
        dev.apply_button(2, true);
        dev.apply_button(1, true);
        let mask = [(1u32 << 2) | (1u32 << 3) | (1u32 << 4)];
        let values = [fp32(2048, 0), fp32(-13, 0x8000_0000), fp32(25, 0)];
        dev.apply_valuators(&mask, &values);

        let first = dev.build_sample(300 * 65_536, 400 * 65_536, Ticks(500));
        assert_eq!(first.position, [300.0, 400.0]);
        assert!((first.pressure.unwrap() - 0.5).abs() < 1e-5);
        assert_eq!(first.tilt, Some([-12.5, 25.0]));
        assert_eq!(
            first.buttons,
            Buttons::SECONDARY
                .union(Buttons::BARREL)
                .union(Buttons::ERASER)
        );
        assert!(first.in_contact);
        assert!(first.inverted);

        // Subsequent pure X/Y motion (`valuator_mask = 0`) retains cached pressure & tilt.
        dev.apply_valuators(&[], &[]);
        let second = dev.build_sample(310 * 65_536, 405 * 65_536, Ticks(600));
        assert!((second.pressure.unwrap() - 0.5).abs() < 1e-5);
        assert_eq!(second.tilt, Some([-12.5, 25.0]));
    }

    #[test]
    fn pads_mice_and_puck_cursors_are_excluded_from_x11_pen_devices() {
        let atoms = sample_atoms();
        for name in [
            &b"xwayland-tablet-pad:15"[..],
            &b"xwayland-tablet cursor:15"[..],
            &b"Wacom Intuos Pro M Pad pad"[..],
            &b"SynPS/2 Synaptics TouchPad"[..],
            &b"Logitech USB Optical Mouse"[..],
        ] {
            let info = XIDeviceInfo {
                deviceid: 12,
                type_: DeviceType::SLAVE_POINTER,
                attachment: 2,
                enabled: true,
                name: name.to_vec(),
                classes: vec![valuator_class(12, 2, atoms.abs_pressure, 0, 2048, 0)],
            };
            assert!(
                X11PenDevice::from_device_info(&info, &atoms).is_none(),
                "should exclude {:?}",
                String::from_utf8_lossy(name)
            );
        }
    }
}
