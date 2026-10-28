# macOS·Linux 지원 계획

2026-10-28 작성. 사용자 요청: Windows만 지원하는 3.x에 macOS와 Linux 지원을 더한다. 이 문서는 계획이다. 정한 것은 구현하면서 decisions.md로, 측정은 evidence.md로 옮기고, 이 문서는 일이 끝나면 지운다(애니메이션 계획과 같은 방식). backlog B3-13(macOS 3.x)은 이 계획이 끝날 때 닫는다.

코드 위치는 main `843b5a4`를 읽은 것이다. 바깥 crate의 상태는 2026-10-28에 crates.io와 각 저장소에서 확인했다(8절 출처). winit·wgpu-hal·arboard가 macOS·Linux에서 하는 일은 고정한 판의 소스를 읽은 것이고, 앱이 아직 그 OS에서 빌드되지 않아 실행해 보지 않았다. 측정하거나 실행해 보지 않은 비용·원인·동작은 **가설**로 적는다.

## 1. 지켜야 할 약속

1. **화소 일치**: 화면·재생·내보내기는 한 `RenderPlan`을 평가하고 원본 크기에서 캔버스와 내보내기가 바이트 같다(decisions.md 2절). 이것은 한 PC 안의 약속이다. 지금 코드는 그보다 넓게 x86 PC끼리도 같은 바이트를 노린다: `raster::document_level`이 AVX2 대신 SSE4.2로 고정한 이유다(다른 제조사 CPU와 libm의 차이는 backlog B3-71). 다른 OS·CPU에서도 같은 바이트인지는 6절에서 정한다.
2. **Windows는 그대로**: 단계마다 Windows의 동작·성능·패키지가 바뀌지 않는다. 회귀 fixture digest, `render-bench`, `latency-probe`, Sandbox `gui` 단계가 전과 같아야 받는다.
3. **성능 우선, 8 작업자 기준**: 새 OS에서도 `ugu-doc render <file> 8`로 재고, 2.2.13 macOS 판이 있는 항목은 그것과 비교한다.
4. **장치 없이 통과로 적지 않는다**: 펜·터치·GPU는 Windows처럼 실제 장치로 확인하기 전까지 열린 gate다(scope.md 2절).
5. **형식은 그대로**: `.ugurugu` 파일은 OS와 관계없이 같다. 경로·줄바꿈·글꼴 이름에 OS가 새지 않는다.

## 2. 지금 상태

evidence.md 97절이 macOS에서 확인한 것이다. 요약:

- OS 의존이 없는 네 crate(`ugu-core`·`ugu-io`·`ugu-session`·`ugu-text`)는 macOS에서 빌드되고, `ugu-text`의 시험 둘만 실패한다. `Fonts::new`의 대체 글꼴이 Windows 글꼴 이름뿐이라 다른 글꼴로 풀린 탓이다(evidence.md 97절에서 확인).
- 나머지는 `windows` crate를 조건 없이 의존해 빌드되지 않는다. 코드에 `cfg(windows)` 분기가 하나도 없다.
- Windows 코드는 거의 `ugu-win`(14개 모듈, 약 2,800줄)에 있고, 그 밖에는 `ugu-render`의 `gpu.rs`·`present.rs`·`view.rs` 시험, 앱의 `app.rs`·`build.rs`·`recovery.rs`·`settings.rs`·`theme.rs`·`render.rs`(`Backends::DX12`), 도구 셋이다. 앱과 도구의 파일 22개가 `ugu_win::`을 부르고, 3.1절대로 하면 이것들은 경로만 바뀐다.

## 3. 구조

### 3.1 플랫폼 경계

`ugu-win`은 Windows 구현으로 남기고, 앱이 OS를 모르게 하는 crate 둘을 사이에 둔다(**제안**, 0단계에서 정해 decisions.md 1절의 crate 행을 고친다). OS 구현은 공통 타입을 쓰고 앱은 구현을 고르는 쪽을 쓴다. 둘을 한 crate에 두면 그 crate와 `ugu-win`이 서로를 의존하게 되어 Cargo가 받지 않는다.

- `ugu-platform-types`: OS와 무관한 타입(`PointerEvent`, `PointerSample`, `PointerKind`, `PointerId`, `Buttons`, `Ticks`, `Placement`, `Memory`, `FileType`·`Dialog` 등). 다른 crate에 의존하지 않는다.
- `ugu-win`(Windows), `ugu-mac`(macOS), `ugu-linux`(Linux): 각 OS의 구현. 타입 crate만 의존하고, `ugu-platform`이 `[target.'cfg(...)'.dependencies]`로 그 OS에서만 끌어온다.
- `ugu-platform`: 타입을 다시 내보내고 대상의 구현을 고르는 얇은 함수. 앱과 도구는 이 crate만 쓴다.
- 타입을 옮기면 Windows 쪽 코드는 `use` 경로만 바뀐다. 동작은 바뀌지 않는다.

다른 안: `ugu-win`을 `ugu-os`로 바꾸고 `cfg` 모듈로 나누기. 이름 바꾸기로 diff가 커져 Windows 검토가 어려워지므로 고르지 않는다.

`hwnd`(`isize`)를 받는 API(`placement`, `instance`, `session`, `pointer`, `dialog`의 owner)는 `&winit::window::Window`나 `raw_window_handle`을 받게 바꾼다(`app.rs` `hwnd_of`). macOS에서 winit은 메인 스레드 밖에서 창 핸들을 주지 않으므로(3.4절), 지금 render 스레드로 넘기는 `hwnd`(`render.rs` `Links`)로 하는 일은 macOS에서 UI 스레드로 보낸다.

### 3.2 GPU와 표시 (`ugu-render`)

- wgpu 기능을 대상마다 나눈다: Windows `dx12`, macOS `metal`, Linux `vulkan`. Windows에 Vulkan을 넣지 않는 이유(decisions.md 2절)는 wgpu 30.0.1에서도 그대로다: 같은 GPU면 Vulkan 어댑터가 먼저 온다. Cargo는 대상별 의존성의 기능을 그 대상에서만 합친다.
- `Backends::DX12`를 쓰는 세 곳(`apps/ugurugu/src/render.rs`, `crates/ugu-render/src/view.rs` 시험, `tools/render-bench/src/gpu.rs`)은 대상의 백엔드를 돌려주는 함수 하나로 바꾼다.
- `gpu.rs`의 DX12 전용(장치 제거 이유, `RemoveDevice` 시험)과 `present.rs`의 프레임 지연 대기·`GetFrameStatistics`는 `cfg(windows)`로 감싼다. 다른 OS는:
  - 장치 손실: `set_device_lost_callback`만 쓴다. 손실을 일부러 일으키는 시험은 Windows에만 둔다.
  - 프레임 지연: `desired_maximum_frame_latency = 1`과 `get_current_texture`의 대기. 입력→표시 지연을 같은 방식으로 재 Windows와 비교한다.
  - 프레임 대기의 상한: wgpu-hal 30.0.1의 Metal은 `allowsNextDrawableTimeout(false)`라 `get_current_texture`가 시간 제한 없이 기다린다. `Presenter::wait_for_frame`은 waitable이 없으면 곧바로 `true`를 돌려주므로, 장치 손실 뒤 프레임이 1초 넘게 막히면 표시를 다시 만드는 장치(`render.rs` `STALL_LIMIT`, decisions.md 2절)가 macOS에서는 걸리지 않는다. 대기를 render 스레드 밖에서 지켜보거나, 가려진 창(`WindowEvent::Occluded`)은 그리지 않게 한다(실제로 오래 막히는 경우가 있는지는 **가설**).
  - 표시 시각: macOS는 `CAMetalDrawable`의 `presentedTime`(wgpu hal로 닿는지 확인 필요), Linux는 없음(`Displayed`를 내지 않고 지연 기록에서 뺀다).
- 소프트웨어 대체(WARP 자리): Linux는 Mesa lavapipe(Vulkan, `DeviceType::Cpu`)를 `force_fallback_adapter`로. macOS는 소프트웨어 Metal이 없지만 지원할 macOS는 모두 Metal이 있어, 대체 없이 실패를 안내한다. `UGURUGU_ADAPTER=warp`는 `fallback`으로 이름을 넓히고 `warp`도 받는다.

### 3.3 모듈별 대응

크기 S·M·L은 일의 양이다. "다름"은 decisions.md에 적을 차이다.

| 모듈 | 하는 일(Windows) | macOS | Linux | 크기, 다름 |
|---|---|---|---|---|
| `pointer` | `WM_POINTER*` 서브클래스, history 정렬·중복 제거, 마우스 이동 복구, 필압·지우개·barrel·hover | 4절 | 4절 | L |
| `clock` | QPC `Ticks`, 지역 시각 문자열 | `mach_absolute_time`(NSEvent·CAMetal과 같은 기준) | `CLOCK_MONOTONIC`(evdev·Wayland와 같은 기준) | S. `Ticks`는 나노초로 통일하거나 OS별 주파수를 둔다 |
| `file` | `ReplaceFileW`(속성·보안 유지)·`MoveFileExW`, 남은 임시 파일의 pid 확인 | 임시 파일 `fsync` 뒤 `rename`, 그 뒤 폴더 `fsync`. 새 대상은 `renamex_np(RENAME_EXCL)`, pid는 `kill(pid, 0)`. 바꿀 파일의 권한·ACL·확장 속성은 `fcopyfile(COPYFILE_METADATA)`로 옮긴다 | 같음, 새 대상은 `renameat2(RENAME_NOREPLACE)`, 권한은 `fchmod`로 옮긴다 | M. 옮기지 않으면 새 파일은 umask의 권한으로 생겨, 600이던 파일이 644가 될 수 있다. Linux의 ACL·확장 속성은 정하지 않았다 |
| 설정·복구 잠금 | `share_mode(0)`와 오류 32(`settings.rs`, `recovery.rs`). 설정 잠금 파일은 닫을 때 지운다(`FILE_FLAG_DELETE_ON_CLOSE`) | `File::try_lock`(Rust 1.89부터 std) | 같음 | M. 권고 잠금이라 "있지만 잠김"을 잠금 결과로만 판단. 잠금은 이름이 아니라 열린 파일에 걸리므로, 잠근 파일을 지우면 기다리던 프로세스는 지워진 파일을, 다음 프로세스는 새 파일을 잠가 둘이 함께 들어간다. 설정 잠금 파일은 지우지 않고 남기거나, 잠근 뒤 경로가 같은 파일(inode)을 가리키는지 본다 |
| `folder` | Roaming·Local AppData, 문서, 탐색기로 열기 | `~/Library/Application Support`, 로그는 `~/Library/Logs`, `open` | `$XDG_CONFIG_HOME`·`$XDG_STATE_HOME`·`$XDG_DATA_HOME`, `xdg-open` | S. Roaming·Local 구분 없음 |
| `dialog` | IFileDialog(자기 STA 스레드), 확장자를 붙인 이름으로 다시 묻기 | `rfd`(NSOpenPanel·NSSavePanel, 메인 스레드) | `rfd`(xdg-desktop-portal) | M. 다시 묻기는 그대로 옮긴다 |
| `clipboard` | 변경 번호, PNG·DIBV5, 읽기 전 크기 확인 | 쓰기는 `arboard`. 읽기는 `NSPasteboard`에서 PNG(없으면 TIFF) 원본을 받는다. 변경 번호는 `NSPasteboard.changeCount` | 쓰기는 `arboard`(X11·XWayland, Wayland는 `wayland-data-control`). 읽기는 PNG 원본(방법은 3단계에서). 변경 번호 없음 | M. arboard 3.6.1의 `get_image`는 macOS에서 TIFF, X11에서 PNG를 다 받아 크기를 보지 않고 RGBA로 디코드한다. 그래서 Windows처럼 원본을 ugu-io의 한도로 디코드한다. Linux는 내용 해시로 "우리 것"을 가린다 |
| `instance` | 이름 있는 mutex, `WM_COPYDATA`로 문서 넘기기, 앞으로 가져오기 | LaunchServices가 한 실행을 지키고 파일은 `application:openURLs:`로 온다. winit의 delegate를 바꿀 수 없어 다른 길로 받는다(3.4절) | 잠금 파일 + Unix 소켓(`$XDG_RUNTIME_DIR`)에 `key\0path`, 앞으로는 xdg-activation | M~L. 창 여럿 중 맨 위 고르기 없음 |
| `session` | `WM_ENDSESSION`에서 복구 쓰기 | `applicationShouldTerminate:`에 `NSTerminateLater`로 답하고 복구를 쓴 뒤 끝낸다(3.4절) | SIGTERM, logind delay inhibitor | M |
| `crash` | 할당 없는 예외 필터가 `crash-…-native.txt` | `sigaction`(SEGV·BUS·ILL·FPE·ABRT, `sigaltstack`), `write`만 | 같음 | M. 예외 코드 대신 신호 번호. Rust std가 스택 넘침을 알리려 SIGSEGV·SIGBUS 처리기를 이미 두므로, 그 처리기를 저장해 두고 기록 뒤 넘긴다 |
| `locale` | 선호 UI 언어, Windows 판 | `sys-locale`, `kern.osproductversion` | `sys-locale`(`ko_KR.UTF-8` → `ko-KR`), `/etc/os-release` | S |
| `memory` | 설치·사용 가능·커밋 여유 | `hw.memsize`, `host_statistics64` | `/proc/meminfo`의 `MemAvailable`. 커밋 여유(`CommitLimit − Committed_AS`)는 `vm.overcommit_memory`가 2일 때만 | S. macOS에는 커밋 여유가 없어 사용 가능 + 스왑 여유로(`budget.rs`의 식을 다시 잰다). Linux 기본값(0)에서는 CommitLimit가 강제되지 않는다. `budget.rs`는 `available.min(commit_available)`를 쓰므로, 커밋 여유가 0에 가까우면 예산이 FLOOR(256MiB)나 캐시가 가진 것의 절반으로 떨어진다. 데스크톱에서 `Committed_AS`가 CommitLimit를 자주 넘는다는 것은 **가설**이다 |
| `placement` | 창 위치·최대화 저장·복원 | winit 위치·크기, 화면 안으로는 우리가 맞춘다 | 같음. Wayland는 위치를 못 정해 크기·최대화만 | S~M |
| `recent` | Jump List | `NSDocumentController noteNewRecentDocumentURL` | `recently-used.xbel` 또는 하지 않음 | S |
| `association` | HKCU 파일 연결(설치 hook) | `Info.plist`의 `CFBundleDocumentTypes`·UTI(빌드 때) | `.desktop` + shared-mime-info XML | M. 실행 중 등록·해제 없음 |

앱 쪽에 더 고칠 것:

- **창 아이콘·클래스**(`app.rs`): Windows는 리소스 1. macOS는 번들 아이콘, Linux는 PNG `Icon::from_rgba`와 app_id(`with_name`).
- **`build.rs`**: `.rc` 리소스는 Windows에서만. macOS 번들의 `Info.plist`·`.icns`는 패키지 단계에서 만든다.
- **글꼴**: `ugu-text` `Fonts::new`의 대체 순서와 `theme.rs`의 중국어 UI 글꼴(`msyh.ttc`)을 OS마다. macOS는 Apple SD Gothic Neo·Hiragino Sans·PingFang SC, Linux는 Noto Sans CJK(fontconfig). 글꼴이 없을 때의 안내도 둔다.
- **단축키**(`shortcuts.rs`): 지금은 `ctrl || command`를 한 "Ctrl"로 보고 "Ctrl+"로 적는다. macOS는 Cmd를 주 수정자로 보이고 ⌘로 적되, 저장 형식(`"ctrl"`)은 OS와 무관한 "주 수정자"로 읽어 설정 파일이 OS를 넘나들어도 같게 한다. Ctrl+Space 끌기 확대처럼 Ctrl 그 자체인 조작은 따로 본다. 클립보드 키를 고르는 `clipboard_chord`는 `control_key()`만 보아 macOS에서 Cmd+C·V·X를 클립보드 키로 보지 않으므로, 이것도 주 수정자로 바꾼다.
- **메뉴**: 처음에는 지금의 창 안 메뉴를 그대로 쓴다. macOS 메뉴 막대(`muda`)는 macOS 전용 의존으로 나중에 본다(Linux 쪽이 GTK를 끌어온다). winit이 만드는 기본 메뉴는 3.4절.
- **파일 끌어놓기**: 앱은 `WindowEvent::DroppedFile`로 연다(`render.rs`). winit 0.30.13은 이 이벤트를 Windows·macOS·X11에서만 내고 Wayland에서는 내지 않는다. Wayland는 `wl_data_device`로 직접 받거나, 안 되는 기능으로 안내한다(3단계).
- **도구**: `ugu-doc`·`render-bench`의 최대 메모리는 `getrusage(RUSAGE_SELF).ru_maxrss`(Linux KiB, macOS 바이트). `latency-probe`는 PresentMon·`SendInput`에 기대므로 Windows에만 빌드한다.

### 3.4 macOS의 메인 스레드와 앱 delegate

winit 0.30.13의 macOS 코드를 읽은 것이다(8절). AppKit은 창과 앱 객체를 메인 스레드에서만 다루게 한다.

- **창 호출**: `Window`의 메서드를 메인 스레드가 아닌 곳에서 부르면 메인 스레드가 그것을 실행할 때까지 기다린다. 큐에 넣는다는 `request_redraw`·`set_cursor`도 이 판에서는 기다린다(`maybe_queue_on_main`). 메인 스레드 밖에서는 창 핸들도 받을 수 없다(`raw_window_handle`이 `Unavailable`). 이 앱은 egui-winit `State`와 창 호출을 "render" 스레드에서 한다(`app.rs`의 render 스레드, `render.rs`의 `on_window_event`). 그래서 macOS에서는 창 호출마다 메인 스레드를 한 번 오가고, 메인 스레드가 render 스레드의 답을 기다리는 동안 render 스레드가 창을 부르면 둘이 서로 기다린다(**가설**: 실행해 보지 않았다). 1단계에서 이 왕복 시간을 재고, 메인 스레드가 render 스레드를 기다리는 곳(종료, 세션 끝)은 기다리지 않는 흐름으로 바꾼다. 왕복이 지연을 늘리면 egui-winit `State`를 UI 스레드로 옮기는 것을 본다.
- **앱 delegate**: winit은 자기 `NSApplicationDelegate`를 등록하고, delegate가 다른 것으로 바뀌면 panic한다. winit의 문서(`platform/macos.rs`)는 delegate를 등록하지 않는다고 적었지만 이 판의 코드와 다르다. 그래서 `application:openURLs:`·`applicationShouldTerminate:`는 delegate를 바꾸지 않고 받는다: winit의 delegate 클래스에 실행 중에 메서드를 더하거나, Apple Event(`kAEOpenDocuments`·`kAEQuitApplication`) 처리기를 직접 등록한다. 처리기를 AppKit보다 늦게, 첫 파일 열기 이벤트보다 먼저 등록할 수 있는지는 2단계에서 확인한다.
- **Cmd+Q**: winit의 기본 메뉴는 Cmd+Q를 `terminate:`에, Cmd+H를 `hide:`에 묶는다. winit delegate에는 `applicationShouldTerminate:`가 없고 `applicationWillTerminate:`는 창을 닫고 이벤트 루프를 끝낼 뿐이라, Cmd+Q는 앱의 "저장하지 않은 변경 먼저" 흐름(`render.rs` `CloseRequested`)을 거치지 않고 끝낼 것이다(**가설**). 그래서 1단계에서 기본 메뉴를 끄거나(`with_default_menu(false)`) Cmd+Q를 앱의 끝내기로 보낸다. 2단계의 종료 hook은 `NSTerminateLater`로 답하고 저장 확인이 끝난 뒤 `replyToApplicationShouldTerminate:`로 끝내, 그동안 메인 스레드를 막지 않는다.

## 4. 펜·터치 입력

2026-10-28 상태:

- winit 최신 판은 0.30.13이고 펜 API가 없다. 0.31.0-beta.4(2026-10-24)에 `PointerKind::TabletTool`과 `TabletToolData`(force·tilt·twist 등)가 있지만, macOS(PR #4593)와 X11(PR #4633)은 아직 열린 PR이고, history(병합된 표본)는 어느 판에도 없다.
- winit에 의존하는 egui-winit은 지금 쓰는 0.36.2도 최신 0.37.0(2026-10-15)도 winit 0.30.13을 요구한다. 그래서 winit 0.31로 갈 수 없다.
- `octotablet`은 macOS가 없고 사실상 멈췄다.

그래서 Windows처럼 OS의 펜 입력을 직접 받는다. 앱이 `PointerSample`에서 읽는 것은 위치·버튼·필압·시각·`inverted`·종류·포인터 id뿐이다(tilt·twist·`frame_id`·`in_contact`는 읽지 않는다). 공통 타입은 그대로 두고 OS가 채울 수 있는 것만 채운다.

| | macOS | Wayland | X11 |
|---|---|---|---|
| 받는 곳 | winit의 `NSView`에 objc2로 이벤트를 먼저 받는다(서브클래스 또는 local event monitor) | `zwp_tablet_v2`(winit과 같은 `wl_display`·`wl_surface`) | XInput2 `XI_Motion`의 valuator |
| 필압 | `NSEvent.pressure`(0~1) | 0~65535 | 장치 valuator 범위 |
| 지우개 끝 | 근접 이벤트의 `pointingDeviceType == Eraser`를 `deviceID`마다 기억한다(점 이벤트에는 이 값이 없다는 것은 **확인 필요**) | 도구 종류 `eraser` | 장치 이름·도구 id(장치마다 다름, **가설**) |
| hover·떠남 | `tabletProximity` | proximity in/out | 근접 valuator(드라이버마다) |
| 병합된 표본 | 없음. `mouseCoalescingEnabled = false`로 모두 받는다(모든 표본이 오는지는 **가설**) | 표본이 `frame`으로 모두 온다 | raw 이벤트 |
| 시각 | NSEvent `timestamp`(mach 기준 초) | ms, `zwp_input_timestamps_v1`이면 ns | ms |
| 터치 | 직접 터치 없음. 트랙패드 두 손가락은 winit의 `PinchGesture`·`RotationGesture`·스크롤 | `wl_touch` | XI2 touch |

마우스·트랙패드만이면 winit 이벤트로 충분하다. 그래서 1단계는 winit의 마우스·휠·터치를 `PointerEvent`로 바꾸는 공통 구현으로 시작하고(필압 없음으로 보고), 2단계에서 macOS 펜, 3단계에서 Linux 펜을 더한다. Windows의 마우스 이동 복구(`GetMouseMovePointsEx`)는 다른 OS에 필요 없다.

egui가 포인터를 받는 길은 OS마다 다르다. Windows에서는 winit이 포인터를 보지 못해 앱이 `PointerEvent`를 egui 이벤트로 바꿔 넘긴다(`input.rs`). macOS와 X11에서는 winit도 같은 이벤트를 받으므로 egui-winit에 포인터 이벤트를 두 번 넘기지 않게 한다. Wayland에서 tablet-v2를 바인딩하면 compositor가 펜을 포인터로 흉내 내지 않아 winit과 egui가 펜을 보지 못할 것이다(**가설**: `tablet-v2.xml`에는 포인터 흉내에 관한 규정이 없어 compositor마다 다를 수 있다). 그때는 Windows처럼 앱이 넘긴다. 커서 모양은 어느 OS든 앱이 직접 적용한다(`render.rs`).

## 5. 파일·경로·설정 위치

| | Windows(지금) | macOS | Linux |
|---|---|---|---|
| 설정 | `%APPDATA%\Ugurugu\3` | `~/Library/Application Support/Ugurugu/3` | `$XDG_CONFIG_HOME/ugurugu/3` |
| 복구 | `%LOCALAPPDATA%\Ugurugu Data\3` | `~/Library/Application Support/Ugurugu Data/3` | `$XDG_STATE_HOME/ugurugu/3` |
| 로그·충돌 기록 | `%LOCALAPPDATA%\Ugurugu Data\3\logs` | `~/Library/Logs/Ugurugu/3` | `$XDG_STATE_HOME/ugurugu/3/logs` |

`UGURUGU_SETTINGS_PATH`·`UGURUGU_RECOVERY_PATH`·`UGURUGU_LOG_PATH`는 모든 OS에서 같다. 2.2.13 macOS 판의 설정·복구는 옮겨 오지 않는다(Windows와 같은 결정, scope.md 5절).

## 6. CPU가 다를 때의 픽셀

evidence.md 97절: ARM의 NEON은 x86 SSE4.2와 바이트가 다르고(시험 장면에서 0.83%, 최대 차 5), 스칼라는 같다. 이때의 x86은 같은 Mac의 Rosetta 2이고 실제 x86 PC가 아니다. 1스레드 중앙값은 NEON 371.0ms, 스칼라 442.9ms로, NEON이 약 16% 짧고 스칼라가 약 19% 느리다. 차이는 NEON의 `mul_add`가 FMA이기 때문으로 본다(**가설**: 코드를 읽은 것, 연산마다 나눠 재지 않았다). 같은 PC 안의 캔버스 = 내보내기는 어느 쪽이든 지켜진다.

x86을 SSE4.2로 고정한 이유도 FMA다(`raster::document_level`: AVX2는 multiply-add를 합쳐 그라디언트 색이 달라지고, SSE4.2는 잰 속도 차이가 없다). A는 같은 이유를 ARM에서는 받아들이지 않는 것이고, 그 근거는 ARM에서는 속도 차이가 잰 만큼 있다는 것이다.

| 안 | 좋은 점 | 대가 |
|---|---|---|
| **A. ARM은 NEON(추천)** | 성능 우선 원칙. 지금 코드 그대로(`document_level`은 ARM에서 이미 NEON) | 회귀 digest(`crates/ugu-render/tests/regression/digests.txt`)를 ARM용으로 하나 더 둔다. 같은 파일을 Mac과 Windows에서 내보내면 1~5단계 다른 화소가 생긴다. `document_level`의 주석("all CPUs give the same pixels")과 decisions.md 2절의 "SSE4.2 고정" 행을 x86에 한정해 고친다 |
| B. ARM은 스칼라 | 모든 OS·CPU에서 같은 바이트, digest 하나 | `fearless_simd`의 `force_support_fallback`을 켜야 하고, ARM에서 래스터가 느려진다 |
| C. ARM은 FMA를 뺀 NEON(실험) | 바이트가 x86과 같아지면 NEON에 가까운 속도에 digest 하나 | `fearless_simd`의 NEON `mul_add`를 곱한 뒤 더하기로 바꾼 판을 `[patch]`로 써야 한다(직접 유지하거나 상류에 선택 기능을 요청). 같은 바이트가 될지는 위의 FMA 가설에 달렸고, 빼면 얼마나 느려지는지는 재야 안다 |

정하기 전에 앱 렌더러로 다시 잰다: 1단계에서 `ugu-doc digest`를 fixture ①~⑤·회귀 fixture로 NEON·스칼라·C의 판으로 돌려 Windows PC의 digest(`digests.txt`)와 비교하고, `ugu-doc render <file> 8`로 시간을 비교한다. C의 비교가 FMA 가설을 바로 확인한다. `f64` `sin`·`cos`·`hypot`이 OS의 libm마다 다를 수 있다는 점(backlog B3-71)도 그때 같이 본다. 그쪽이 다르면 B·C를 골라도 OS 사이 바이트가 같지 않다.

ARM에서는 `raster.rs`의 시험 `every_simd_level_draws_the_same_pixels`가 비교하는 것이 없다: `levels()`에 aarch64 분기가 없어 baseline 하나만 돈다. ARM에서 문서를 그리는 수준은 어느 안이든 하나뿐이라 그것이 맞지만, ARM의 바이트는 회귀 digest만 지킨다.

## 7. 단계

단계마다 따로 PR(필요하면 여럿을 쌓아서)로 내고, 각 단계의 끝에 Windows 확인(1절 2)을 한다.

### 0단계: 세 OS에서 빌드

- 작업 공간의 `windows`·`windows-sys`·`clipboard-win`·`embed-resource`를 대상별 의존으로, wgpu 기능을 대상별로(3.2절).
- `ugu-platform-types`·`ugu-platform`을 만들고(3.1절) 공통 타입을 옮긴다. Windows 밖에서는 모든 OS 함수가 "지원하지 않음"을 돌려주는 자리만 둔다.
- `ugu-render`의 DX12 코드를 `cfg(windows)`로.
- `build.rs`·`latency-probe`·Windows 전용 시험(`C:\` 경로, 공유 위반, 레지스트리)을 `cfg(windows)`로.
- `ugu-text`·`theme.rs`의 OS별 글꼴(시험이 macOS에서 통과).
- `rust-toolchain.toml`의 `targets`는 그대로 둔다. 각 OS의 host target은 적지 않아도 설치되고, 적으면 Windows 개발 PC도 쓰지 않을 표준 라이브러리를 받는다. Windows에서 다른 OS를 대상으로 `cargo check`하는 것도 libwebp-sys의 C 빌드에서 막힐 것이다(**가설**).
- CI: macOS(arm64)와 Ubuntu에서 `cargo clippy --workspace --all-targets -D warnings`와 `cargo test --workspace`. GPU 시험은 어댑터가 없으면 "skipped: no DX12 adapter"를 찍고 통과하므로(`view.rs`·`ants.rs`), 그대로 두면 새 OS에서는 아무것도 재지 않고 통과한다. Ubuntu는 lavapipe를 깔고 macOS는 runner의 Metal을 써서(runner에서 Metal 어댑터가 열리는지는 **확인 필요**) 어댑터가 열리게 하고, 건너뛴 시험이 있으면 CI를 실패시킨다(**제안**). 메시지의 "DX12"는 백엔드 이름으로 바꾼다. `tools/native_deps.py`의 승인 목록을 OS마다 확인한다.

끝: 세 OS에서 빌드·clippy·시험이 통과하고, Windows의 digest·패키지·Sandbox `gui`가 그대로.

상태(2026-10-28): 구현은 쌓은 PR #135(플랫폼 crate)·#136(대상별 의존·cfg)·#137(글꼴)·#138(CI)에 있다. Mac에서 확인한 것: macOS clippy·시험(Metal 어댑터로 GPU 시험 포함), Ubuntu 24.04 arm64 Docker의 clippy·시험(lavapipe), `x86_64-pc-windows-gnu` 교차 clippy. #135~#138의 CI는 모든 job이 통과했다. #138의 macOS job(`macos-26-arm64`)과 Linux job(`ubuntu-24.04`, lavapipe)에서 `UGU_REQUIRE_ADAPTER`를 켠 채 `view`·`ants`의 GPU 시험이 건너뛰지 않고 통과했으니 두 runner에서 어댑터가 열린다. 남은 것:

- Windows PC에서 `render-bench`·`latency-probe`·Sandbox `gui`를 전과 비교.
- 계획과 달라진 것: 파일 교체와 복구·설정 잠금은 "지원하지 않음" 대신 Unix 구현을 앞당겼다(앱 시험이 저장·복구·설정에 기대므로, decisions.md 7절). 그 밖의 OS 함수는 `crates/ugu-platform/src/unsupported.rs`의 자리표시이고, 1단계가 이것을 macOS 구현으로 바꾼다. `hwnd`(`isize`)를 받는 API는 아직 그대로다(3.1절). Windows 밖의 `Ticks`는 표준 라이브러리 단조 시계의 나노초다.
- 글꼴이 없을 때의 안내(3.3절)는 하지 않았다.

### 1단계: macOS에서 마우스·트랙패드로 그리기

- Metal 표시, 장치 손실 복구, 프레임 지연, 막힌 프레임 대기의 복구(3.2절).
- 기본 메뉴의 Cmd+Q가 저장 확인을 건너뛰지 않게 하고, 메인 스레드가 render 스레드를 기다리는 곳을 없앤다(3.4절).
- winit 이벤트를 `PointerEvent`로(4절). 트랙패드 두 손가락 팬·핀치·회전.
- `file`·잠금·`folder`·`dialog`·`clipboard`·`locale`·`memory`·`placement`·`crash`·`clock`(3.3절). 단일 실행은 우선 잠금만(두 번째 실행은 끝냄).
- Cmd 단축키와 표시.
- 측정: fixture ①~⑤ `ugu-doc render <file> 8`, 6절의 digest 비교(NEON·스칼라·C), 창 호출의 메인 스레드 왕복 시간(3.4절), 2.2.13 macOS 판과 같은 방식의 획 지연(macOS용 측정 도구가 필요하다, `CGEventPost`로 입력, 표시 시각은 3.2절).
- 개발용 실행: `.app` 번들 없이 `cargo run`으로도 창·입력이 동작하는지 확인한다(번들이 아니면 메뉴·Dock 아이콘이 다르게 보일 수 있다, **가설**).

끝: macOS에서 새 문서·그리기·저장·열기·내보내기·자동 복구·붙여넣기가 동작하고, 6절의 결정을 decisions.md에 적는다.

### 2단계: macOS 펜과 OS 연동

- NSEvent 펜(필압·근접 이벤트로 아는 지우개 끝·hover·barrel, 4절).
- Finder·Dock에서 파일 열기(`application:openURLs:`), 최근 문서, 종료 hook(`applicationShouldTerminate:`, `NSTerminateLater`). winit의 delegate를 바꾸지 않는 방법으로 받는다(3.4절).
- 펜은 장치로 확인하기 전까지 열린 gate다. 합성 입력(`CGEventCreateMouseEvent`에 tablet 필드)으로 경로를 시험할 수 있는지 본다.

### 3단계: Linux

- Vulkan 표시와 lavapipe 대체. 작업 공간이 winit의 기본 기능을 끄므로 `x11`·`wayland`에 더해 `wayland-dlopen`(libwayland를 실행 때 열어, 그것이 없는 시스템에서도 X11로 뜬다)과 `wayland-csd-adwaita`를 켠다. GNOME은 서버 쪽 창 테두리가 없어, 이 기능이 없으면 winit은 글자 없는 sctk `FallbackFrame`을 그린다.
- 포털 대화상자, 클립보드(X11·Wayland), Unix 소켓 단일 실행과 문서 넘기기, SIGTERM·logind, 파일 연결(`.desktop`, MIME), Wayland의 파일 끌어놓기(3.3절), 메모리 예산의 커밋 여유(3.3절 `memory`).
- Wayland tablet-v2(펜을 egui로 넘기기 포함, 4절), X11 XInput2 펜.
- 시험 환경: Ubuntu에서 Xvfb와 weston headless, lavapipe(wgpu CI와 같은 방식). 실제 GPU·펜은 열린 gate.

### 4단계: 패키지·업데이트·릴리스

- Velopack 1.2.161(지금 판)은 macOS `.app`·`.pkg`·`.zip`과 Linux `.AppImage`를 만든다. 채널은 `osx-arm64`·`linux-x64`처럼 나누고, 패키지 ID는 `Ugurugu3` 그대로(**확인 필요**: 한 피드에 여러 채널을 두는 방식).
- macOS 서명: 9절 결정 3에 따른다. Developer ID가 없으면 ad-hoc 서명만이다. 링커의 서명은 실행 파일만 덮으므로 `Info.plist`·자원을 넣은 뒤 번들 전체를 `codesign --force --deep -s -`로 다시 서명한다(하지 않으면 받은 앱이 "손상됨"으로 나와 "그래도 열기"도 없을 수 있다, **가설**). 받은 앱은 macOS 15부터 시스템 설정 > 개인정보 보호 및 보안 > "그래도 열기"로만 연다. Velopack 업데이트는 Developer ID 서명을 전제로 한다(**확인 필요**: 서명 없을 때 업데이트가 되는지).
- Linux: AppImage 하나. glibc 하한을 정한다(빌드 이미지로).
- 제3자 고지(`tools/third_party_notices.py`)를 OS마다 링크되는 crate로.
- `release-3.yml`에 macOS·Linux job, 다운로드 배지·릴리스 노트의 파일 이름.
- 사용자 안내: README(ko·en·ja)의 요구 사항, `BUILDING.md`의 macOS·Linux 빌드 절, 릴리스 노트.

## 8. 바깥 상태 출처 (2026-10-28 확인)

- winit 0.31.0-beta.4, 펜 API PR #4318(병합), AppKit 펜 PR #4593·X11 PR #4633·history PR #4696(열림): <https://github.com/rust-windowing/winit>
- egui-winit 0.36.2·0.37.0이 `winit 0.30.13`을 요구: crates.io
- wgpu 30.0.1의 HAL 순서(Vulkan, Metal, Dx12)와 어댑터 정렬: `wgpu-core/src/instance.rs`
- wgpu-hal 30.0.1 `src/metal/surface.rs`: `setAllowsNextDrawableTimeout(false)`, `maximumDrawableCount`는 지연 + 1
- winit 0.30.13 소스: 메인 스레드 밖의 창 호출(`platform_impl/macos/window.rs` `maybe_wait_on_main`), 앱 delegate(`platform_impl/macos/event_loop.rs`의 `setDelegate`, `app_state.rs` `ApplicationDelegate::get`), 기본 메뉴(`platform_impl/macos/menu.rs`), `DroppedFile`을 내는 곳(Windows `drop_handler.rs`, macOS `window_delegate.rs`, X11 `event_processor.rs`), Wayland 창 테두리(`platform_impl/linux/wayland/window/state.rs`), 기능(`Cargo.toml`)
- arboard 3.6.1의 `get_image`: `src/platform/osx.rs`(TIFF), `src/platform/linux/x11.rs`(PNG)
- Velopack macOS·Linux: <https://docs.velopack.io/packaging/operating-systems/macos>, <https://docs.velopack.io/packaging/operating-systems/linux>, 서명 <https://docs.velopack.io/packaging/signing>
- rfd 0.17.2(기본 xdg-portal), arboard 3.7.0(Wayland는 `wayland-data-control`), muda 0.21.2: crates.io
- GitHub Actions `macos-latest`는 macOS 26 arm64, `ubuntu-latest`는 24.04: <https://github.com/actions/runner-images>

## 9. 남은 사용자 결정

1. **ARM의 SIMD**(6절): A(NEON, 추천), B(스칼라), C(FMA를 뺀 NEON, 실험이 맞을 때). 1단계의 앱 측정 뒤 정한다.
2. **Mac 하드웨어**: Apple Silicon만(추천). Apple은 macOS 26을 Intel Mac의 마지막 판으로 밝혔고, Intel까지 하면 SIMD·digest 조합이 하나 더 는다.
3. **macOS 서명**: 유료 Apple Developer 계정(Developer ID 서명·공증)을 쓸지. 업데이트와 첫 실행 안내가 이것에 달렸다.
4. **Linux 범위**: x64 AppImage, X11·Wayland 둘 다(제안). 배포판 패키지(deb·rpm·Flatpak)는 범위 밖으로 두고 나중에 본다.
5. **지원 OS 하한**: macOS 몇 판부터, Linux glibc 몇부터. 1단계 전에 정한다.
