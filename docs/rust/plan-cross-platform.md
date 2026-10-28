# macOS·Linux 지원 계획

2026-10-28 작성. 사용자 요청: Windows만 지원하는 3.x에 macOS와 Linux 지원을 더한다. 이 문서는 계획이다. 정한 것은 구현하면서 decisions.md로, 측정은 evidence.md로 옮기고, 이 문서는 일이 끝나면 지운다(애니메이션 계획과 같은 방식). backlog B3-13(macOS 3.x)은 이 계획이 끝날 때 닫는다.

코드 위치는 main `843b5a4`를 읽은 것이다. 바깥 crate의 상태는 2026-10-28에 crates.io와 각 저장소에서 확인했다(8절 출처). 측정하지 않은 비용·원인은 **가설**로 적는다.

## 1. 지켜야 할 약속

1. **화소 일치**: 화면·재생·내보내기는 한 `RenderPlan`을 평가하고 원본 크기에서 캔버스와 내보내기가 바이트 같다(decisions.md 2절). 이것은 한 PC 안의 약속이고, 다른 OS·CPU에서도 같은 바이트인지는 6절에서 따로 정한다.
2. **Windows는 그대로**: 단계마다 Windows의 동작·성능·패키지가 바뀌지 않는다. 회귀 fixture digest, `render-bench`, `latency-probe`, Sandbox `gui` 단계가 전과 같아야 받는다.
3. **성능 우선, 8 작업자 기준**: 새 OS에서도 `ugu-doc render <file> 8`로 재고, 2.2.13 macOS 판이 있는 항목은 그것과 비교한다.
4. **장치 없이 통과로 적지 않는다**: 펜·터치·GPU는 Windows처럼 실제 장치로 확인하기 전까지 열린 gate다(scope.md 2절).
5. **형식은 그대로**: `.ugurugu` 파일은 OS와 관계없이 같다. 경로·줄바꿈·글꼴 이름에 OS가 새지 않는다.

## 2. 지금 상태

evidence.md 97절이 macOS에서 확인한 것이다. 요약:

- 외부 의존 없는 네 crate(`ugu-core`·`ugu-io`·`ugu-session`·`ugu-text`)는 macOS에서 빌드되고, `ugu-text`의 시험 둘만 Windows 글꼴 이름 때문에 실패한다.
- 나머지는 `windows` crate를 조건 없이 의존해 빌드되지 않는다. 코드에 `cfg(windows)` 분기가 하나도 없다.
- Windows 코드는 거의 `ugu-win`(14개 모듈, 약 2,800줄)에 있고, 그 밖에는 `ugu-render`의 `gpu.rs`·`present.rs`·`view.rs` 시험, 앱의 `app.rs`·`build.rs`·`recovery.rs`·`settings.rs`·`theme.rs`, 도구 셋이다.

## 3. 구조

### 3.1 플랫폼 경계

`ugu-win`은 Windows 구현으로 남기고, 앱이 OS를 모르게 하는 crate 하나를 사이에 둔다(**제안**, 0단계에서 정해 decisions.md 1절의 crate 행을 고친다).

- `ugu-platform`: OS와 무관한 타입(`PointerEvent`, `PointerSample`, `PointerKind`, `PointerId`, `Buttons`, `Ticks`, `Placement`, `Memory`, `FileType`·`Dialog` 등)과, OS별 구현을 고르는 얇은 함수. 앱과 도구는 이 crate만 쓴다.
- `ugu-win`(Windows), `ugu-mac`(macOS), `ugu-linux`(Linux): 각 OS의 구현. `[target.'cfg(...)'.dependencies]`로 그 OS에서만 빌드한다.
- 타입을 옮기면 Windows 쪽 코드는 `use` 경로만 바뀐다. 동작은 바뀌지 않는다.

다른 안: `ugu-win`을 `ugu-os`로 바꾸고 `cfg` 모듈로 나누기. 이름 바꾸기로 diff가 커져 Windows 검토가 어려워지므로 고르지 않는다.

`hwnd`(`isize`)를 받는 API(`placement`, `instance`, `session`, `pointer`, `dialog`의 owner)는 `&winit::window::Window`나 `raw_window_handle`을 받게 바꾼다(`app.rs` `hwnd_of`).

### 3.2 GPU와 표시 (`ugu-render`)

- wgpu 기능을 대상마다 나눈다: Windows `dx12`, macOS `metal`, Linux `vulkan`. Windows에 Vulkan을 넣지 않는 이유(decisions.md 2절)는 wgpu 30.0.1에서도 그대로다: 같은 GPU면 Vulkan 어댑터가 먼저 온다. Cargo는 대상별 의존성의 기능을 그 대상에서만 합친다.
- `Backends::DX12`를 쓰는 세 곳(`apps/ugurugu/src/render.rs`, `crates/ugu-render/src/view.rs` 시험, `tools/render-bench/src/gpu.rs`)은 대상의 백엔드를 돌려주는 함수 하나로 바꾼다.
- `gpu.rs`의 DX12 전용(장치 제거 이유, `RemoveDevice` 시험)과 `present.rs`의 프레임 지연 대기·`GetFrameStatistics`는 `cfg(windows)`로 감싼다. 다른 OS는:
  - 장치 손실: `set_device_lost_callback`만 쓴다. 손실을 일부러 일으키는 시험은 Windows에만 둔다.
  - 프레임 지연: `desired_maximum_frame_latency = 1`과 `get_current_texture`의 대기. 입력→표시 지연을 같은 방식으로 재 Windows와 비교한다.
  - 표시 시각: macOS는 `CAMetalDrawable`의 `presentedTime`(wgpu hal로 닿는지 확인 필요), Linux는 없음(`Displayed`를 내지 않고 지연 기록에서 뺀다).
- 소프트웨어 대체(WARP 자리): Linux는 Mesa lavapipe(Vulkan, `DeviceType::Cpu`)를 `force_fallback_adapter`로. macOS는 소프트웨어 Metal이 없지만 지원할 macOS는 모두 Metal이 있어, 대체 없이 실패를 안내한다. `UGURUGU_ADAPTER=warp`는 `fallback`으로 이름을 넓히고 `warp`도 받는다.

### 3.3 모듈별 대응

크기 S·M·L은 일의 양이다. "다름"은 decisions.md에 적을 차이다.

| 모듈 | 하는 일(Windows) | macOS | Linux | 크기, 다름 |
|---|---|---|---|---|
| `pointer` | `WM_POINTER*` 서브클래스, history 정렬·중복 제거, 마우스 이동 복구, 필압·지우개·barrel·hover | 4절 | 4절 | L |
| `clock` | QPC `Ticks`, 지역 시각 문자열 | `mach_absolute_time`(NSEvent·CAMetal과 같은 기준) | `CLOCK_MONOTONIC`(evdev·Wayland와 같은 기준) | S. `Ticks`는 나노초로 통일하거나 OS별 주파수를 둔다 |
| `file` | `ReplaceFileW`·`MoveFileExW`, 남은 임시 파일의 pid 확인 | `rename` 뒤 파일·폴더 `fsync`, 새 대상은 `renamex_np(RENAME_EXCL)`, pid는 `kill(pid, 0)` | 같음, 새 대상은 `renameat2(RENAME_NOREPLACE)` | M. ACL·속성 보존은 없다 |
| 설정·복구 잠금 | `share_mode(0)`와 오류 32(`settings.rs`, `recovery.rs`) | `File::try_lock`(Rust 1.89부터 std) | 같음 | M. 권고 잠금이라 "있지만 잠김"을 잠금 결과로만 판단 |
| `folder` | Roaming·Local AppData, 문서, 탐색기로 열기 | `~/Library/Application Support`, 로그는 `~/Library/Logs`, `open` | `$XDG_CONFIG_HOME`·`$XDG_STATE_HOME`·`$XDG_DATA_HOME`, `xdg-open` | S. Roaming·Local 구분 없음 |
| `dialog` | IFileDialog(자기 STA 스레드), 확장자를 붙인 이름으로 다시 묻기 | `rfd`(NSOpenPanel·NSSavePanel, 메인 스레드) | `rfd`(xdg-desktop-portal) | M. 다시 묻기는 그대로 옮긴다 |
| `clipboard` | 변경 번호, PNG·DIBV5, 읽기 전 크기 확인 | `arboard`, 변경 번호는 `NSPasteboard.changeCount` | `arboard`(X11·XWayland, Wayland는 `wayland-data-control`), 변경 번호 없음 | M. Linux는 내용 해시로 "우리 것"을 가린다 |
| `instance` | 이름 있는 mutex, `WM_COPYDATA`로 문서 넘기기, 앞으로 가져오기 | LaunchServices가 한 실행을 지키고 파일은 `application:openURLs:`로 온다(objc2 delegate 필요) | 잠금 파일 + Unix 소켓(`$XDG_RUNTIME_DIR`)에 `key\0path`, 앞으로는 xdg-activation | M~L. 창 여럿 중 맨 위 고르기 없음 |
| `session` | `WM_ENDSESSION`에서 복구 쓰기 | `applicationShouldTerminate:` | SIGTERM, logind delay inhibitor | M |
| `crash` | 할당 없는 예외 필터가 `crash-…-native.txt` | `sigaction`(SEGV·BUS·ILL·FPE·ABRT, `sigaltstack`), `write`만 | 같음 | M. 예외 코드 대신 신호 번호 |
| `locale` | 선호 UI 언어, Windows 판 | `sys-locale`, `kern.osproductversion` | `sys-locale`(`ko_KR.UTF-8` → `ko-KR`), `/etc/os-release` | S |
| `memory` | 설치·사용 가능·커밋 여유 | `hw.memsize`, `host_statistics64` | `/proc/meminfo`(`MemAvailable`, `CommitLimit − Committed_AS`) | S. macOS에는 커밋 여유가 없어 사용 가능 + 스왑 여유로(`budget.rs`의 식을 다시 잰다) |
| `placement` | 창 위치·최대화 저장·복원 | winit 위치·크기, 화면 안으로는 우리가 맞춘다 | 같음. Wayland는 위치를 못 정해 크기·최대화만 | S~M |
| `recent` | Jump List | `NSDocumentController noteNewRecentDocumentURL` | `recently-used.xbel` 또는 하지 않음 | S |
| `association` | HKCU 파일 연결(설치 hook) | `Info.plist`의 `CFBundleDocumentTypes`·UTI(빌드 때) | `.desktop` + shared-mime-info XML | M. 실행 중 등록·해제 없음 |

앱 쪽에 더 고칠 것:

- **창 아이콘·클래스**(`app.rs`): Windows는 리소스 1. macOS는 번들 아이콘, Linux는 PNG `Icon::from_rgba`와 app_id(`with_name`).
- **`build.rs`**: `.rc` 리소스는 Windows에서만. macOS 번들의 `Info.plist`·`.icns`는 패키지 단계에서 만든다.
- **글꼴**: `ugu-text` `Fonts::new`의 대체 순서와 `theme.rs`의 중국어 UI 글꼴(`msyh.ttc`)을 OS마다. macOS는 Apple SD Gothic Neo·Hiragino Sans·PingFang SC, Linux는 Noto Sans CJK(fontconfig). 글꼴이 없을 때의 안내도 둔다.
- **단축키**(`shortcuts.rs`): 지금은 `ctrl || command`를 한 "Ctrl"로 보고 "Ctrl+"로 적는다. macOS는 Cmd를 주 수정자로 보이고 ⌘로 적되, 저장 형식(`"ctrl"`)은 OS와 무관한 "주 수정자"로 읽어 설정 파일이 OS를 넘나들어도 같게 한다. Ctrl+Space 끌기 확대처럼 Ctrl 그 자체인 조작은 따로 본다.
- **메뉴**: 처음에는 지금의 창 안 메뉴를 그대로 쓴다. macOS 메뉴 막대(`muda`)는 macOS 전용 의존으로 나중에 본다(Linux 쪽이 GTK를 끌어온다).
- **도구**: `ugu-doc`·`render-bench`의 최대 메모리는 `getrusage(RUSAGE_SELF).ru_maxrss`(Linux KiB, macOS 바이트). `latency-probe`는 PresentMon·`SendInput`에 기대므로 Windows에만 빌드한다.

## 4. 펜·터치 입력

2026-10-28 상태:

- winit 최신 판은 0.30.13이고 펜 API가 없다. 0.31.0-beta.4(2026-10-24)에 `PointerKind::TabletTool`과 `TabletToolData`(force·tilt·twist 등)가 있지만, macOS(PR #4593)와 X11(PR #4633)은 아직 열린 PR이고, history(병합된 표본)는 어느 판에도 없다.
- egui 0.37.0과 egui-winit 0.36.2는 winit 0.30에 묶여 있다. 그래서 winit 0.31로 갈 수 없다.
- `octotablet`은 macOS가 없고 사실상 멈췄다.

그래서 Windows처럼 OS의 펜 입력을 직접 받는다. 앱이 `PointerSample`에서 읽는 것은 위치·버튼·필압·시각·`inverted`·종류·포인터 id뿐이다(tilt·twist·`frame_id`·`in_contact`는 읽지 않는다). 공통 타입은 그대로 두고 OS가 채울 수 있는 것만 채운다.

| | macOS | Wayland | X11 |
|---|---|---|---|
| 받는 곳 | winit의 `NSView`에 objc2로 이벤트를 먼저 받는다(서브클래스 또는 local event monitor) | `zwp_tablet_v2`(winit과 같은 `wl_display`·`wl_surface`) | XInput2 `XI_Motion`의 valuator |
| 필압 | `NSEvent.pressure`(0~1) | 0~65535 | 장치 valuator 범위 |
| 지우개 끝 | `pointingDeviceType == Eraser` | 도구 종류 `eraser` | 장치 이름·도구 id(장치마다 다름, **가설**) |
| hover·떠남 | `tabletProximity` | proximity in/out | 근접 valuator(드라이버마다) |
| 병합된 표본 | 없음. `mouseCoalescingEnabled = false`로 모두 받는다(모든 표본이 오는지는 **가설**) | 표본이 `frame`으로 모두 온다 | raw 이벤트 |
| 시각 | NSEvent `timestamp`(mach 기준 초) | ms, `zwp_input_timestamps_v1`이면 ns | ms |
| 터치 | 직접 터치 없음. 트랙패드 두 손가락은 winit의 `PinchGesture`·`RotationGesture`·스크롤 | `wl_touch` | XI2 touch |

마우스·트랙패드만이면 winit 이벤트로 충분하다. 그래서 1단계는 winit의 마우스·휠·터치를 `PointerEvent`로 바꾸는 공통 구현으로 시작하고(필압 없음으로 보고), 2단계에서 macOS 펜, 3단계에서 Linux 펜을 더한다. Windows의 마우스 이동 복구(`GetMouseMovePointsEx`)는 다른 OS에 필요 없다.

winit이 포인터를 보지 못하는 Windows와 달리 다른 OS에서는 winit도 같은 이벤트를 받는다. 커서 모양은 Windows처럼 앱이 직접 적용하고(`render.rs`), egui-winit에 포인터 이벤트를 두 번 넘기지 않게 한다.

## 5. 파일·경로·설정 위치

| | Windows(지금) | macOS | Linux |
|---|---|---|---|
| 설정 | `%APPDATA%\Ugurugu\3` | `~/Library/Application Support/Ugurugu/3` | `$XDG_CONFIG_HOME/ugurugu/3` |
| 복구 | `%LOCALAPPDATA%\Ugurugu Data\3` | `~/Library/Application Support/Ugurugu Data/3` | `$XDG_STATE_HOME/ugurugu/3` |
| 로그·충돌 기록 | `%LOCALAPPDATA%\Ugurugu Data\3\logs` | `~/Library/Logs/Ugurugu/3` | `$XDG_STATE_HOME/ugurugu/3/logs` |

`UGURUGU_SETTINGS_PATH`·`UGURUGU_RECOVERY_PATH`·`UGURUGU_LOG_PATH`는 모든 OS에서 같다. 2.2.13 macOS 판의 설정·복구는 옮겨 오지 않는다(Windows와 같은 결정, scope.md 5절).

## 6. CPU가 다를 때의 픽셀

evidence.md 97절: ARM의 NEON은 `mul_add`가 FMA라 x86 SSE4.2와 바이트가 다르고(시험 장면에서 0.83%, 최대 차 5), 스칼라는 x86과 같지만 1스레드 약 16% 느리다. 같은 PC 안의 캔버스 = 내보내기는 어느 쪽이든 지켜진다.

| 안 | 좋은 점 | 대가 |
|---|---|---|
| **A. ARM은 NEON(추천)** | 성능 우선 원칙. 지금 코드 그대로(`document_level`은 ARM에서 이미 NEON) | 회귀 digest(`crates/ugu-render/tests/digests.txt`)를 ARM용으로 하나 더 둔다. 같은 파일을 Mac과 Windows에서 내보내면 1~5단계 다른 화소가 생긴다 |
| B. ARM은 스칼라 | 모든 OS·CPU에서 같은 바이트, digest 하나 | `fearless_simd`의 `force_support_fallback`을 켜야 하고, ARM에서 래스터가 느려진다 |

정하기 전에 앱 렌더러로 다시 잰다: 1단계에서 `ugu-doc digest`를 fixture ①~⑤·회귀 fixture로 NEON과 스칼라 모두 돌리고, `ugu-doc render <file> 8`로 시간을 비교한다. `f64` `sin`·`cos`·`hypot`이 OS의 libm마다 다를 수 있다는 점(backlog B3-71)도 그때 같이 본다. 그쪽이 다르면 B를 골라도 OS 사이 바이트가 같지 않다.

## 7. 단계

단계마다 따로 PR(필요하면 여럿을 쌓아서)로 내고, 각 단계의 끝에 Windows 확인(1절 2)을 한다.

### 0단계: 세 OS에서 빌드

- 작업 공간의 `windows`·`windows-sys`·`clipboard-win`·`embed-resource`를 대상별 의존으로, wgpu 기능을 대상별로(3.2절).
- `ugu-platform`을 만들고 공통 타입을 옮긴다. Windows 밖에서는 모든 OS 함수가 "지원하지 않음"을 돌려주는 자리만 둔다.
- `ugu-render`의 DX12 코드를 `cfg(windows)`로.
- `build.rs`·`latency-probe`·Windows 전용 시험(`C:\` 경로, 공유 위반, 레지스트리)을 `cfg(windows)`로.
- `ugu-text`·`theme.rs`의 OS별 글꼴(시험이 macOS에서 통과).
- `rust-toolchain.toml`의 `targets`에 `aarch64-apple-darwin`·`x86_64-unknown-linux-gnu`.
- CI: macOS(arm64)와 Ubuntu에서 `cargo clippy --workspace --all-targets -D warnings`와 `cargo test --workspace`(GPU 없는 시험만). `tools/native_deps.py`의 승인 목록을 OS마다 확인한다.

끝: 세 OS에서 빌드·clippy·시험이 통과하고, Windows의 digest·패키지·Sandbox `gui`가 그대로.

### 1단계: macOS에서 마우스·트랙패드로 그리기

- Metal 표시, 장치 손실 복구, 프레임 지연(3.2절).
- winit 이벤트를 `PointerEvent`로(4절). 트랙패드 두 손가락 팬·핀치·회전.
- `file`·잠금·`folder`·`dialog`·`clipboard`·`locale`·`memory`·`placement`·`crash`·`clock`(3.3절). 단일 실행은 우선 잠금만(두 번째 실행은 끝냄).
- Cmd 단축키와 표시.
- 측정: fixture ①~⑤ `ugu-doc render <file> 8`, 6절의 digest 비교, 2.2.13 macOS 판과 같은 방식의 획 지연(macOS용 측정 도구가 필요하다, `CGEventPost`로 입력, 표시 시각은 3.2절).
- 개발용 실행: `.app` 번들 없이 `cargo run`으로도 창·입력이 동작하는지 확인한다(번들이 아니면 메뉴·Dock 아이콘이 다르게 보일 수 있다, **가설**).

끝: macOS에서 새 문서·그리기·저장·열기·내보내기·자동 복구·붙여넣기가 동작하고, 6절의 결정을 decisions.md에 적는다.

### 2단계: macOS 펜과 OS 연동

- NSEvent 펜(필압·지우개 끝·hover·barrel, 4절).
- Finder·Dock에서 파일 열기(`application:openURLs:`), 최근 문서, 종료 hook(`applicationShouldTerminate:`).
- 펜은 장치로 확인하기 전까지 열린 gate다. 합성 입력(`CGEventCreateMouseEvent`에 tablet 필드)으로 경로를 시험할 수 있는지 본다.

### 3단계: Linux

- Vulkan 표시와 lavapipe 대체, winit `x11`·`wayland` 기능.
- 포털 대화상자, 클립보드(X11·Wayland), Unix 소켓 단일 실행과 문서 넘기기, SIGTERM·logind, 파일 연결(`.desktop`, MIME).
- Wayland tablet-v2, X11 XInput2 펜.
- 시험 환경: Ubuntu에서 Xvfb와 weston headless, lavapipe(wgpu CI와 같은 방식). 실제 GPU·펜은 열린 gate.

### 4단계: 패키지·업데이트·릴리스

- Velopack 1.2.161(지금 판)은 macOS `.app`·`.pkg`·`.zip`과 Linux `.AppImage`를 만든다. 채널은 `osx-arm64`·`linux-x64`처럼 나누고, 패키지 ID는 `Ugurugu3` 그대로(**확인 필요**: 한 피드에 여러 채널을 두는 방식).
- macOS 서명: 7절 결정 3에 따른다. Developer ID가 없으면 ad-hoc 서명(링커가 이미 한다)만이고, 받은 앱은 macOS 15부터 시스템 설정 > 개인정보 보호 및 보안 > "그래도 열기"로만 연다. Velopack 업데이트는 Developer ID 서명을 전제로 한다(**확인 필요**: 서명 없을 때 업데이트가 되는지).
- Linux: AppImage 하나. glibc 하한을 정한다(빌드 이미지로).
- 제3자 고지(`tools/third_party_notices.py`)를 OS마다 링크되는 crate로.
- `release-3.yml`에 macOS·Linux job, 다운로드 배지·릴리스 노트의 파일 이름.
- 사용자 안내: README(ko·en·ja)의 요구 사항, `BUILDING.md`의 macOS·Linux 빌드 절, 릴리스 노트.

## 8. 바깥 상태 출처 (2026-10-28 확인)

- winit 0.31.0-beta.4, 펜 API PR #4318(병합), AppKit 펜 PR #4593·X11 PR #4633·history PR #4696(열림): <https://github.com/rust-windowing/winit>
- egui 0.37.0이 `winit ^0.30.13`을 요구: crates.io
- wgpu 30.0.1의 HAL 순서(Vulkan, Metal, Dx12)와 어댑터 정렬: `wgpu-core/src/instance.rs`
- Velopack macOS·Linux: <https://docs.velopack.io/packaging/operating-systems/macos>, <https://docs.velopack.io/packaging/operating-systems/linux>, 서명 <https://docs.velopack.io/packaging/signing>
- rfd 0.17.2(기본 xdg-portal), arboard 3.7.0(Wayland는 `wayland-data-control`), muda 0.21.2: crates.io
- GitHub Actions `macos-latest`는 macOS 26 arm64, `ubuntu-latest`는 24.04: <https://github.com/actions/runner-images>

## 9. 남은 사용자 결정

1. **ARM의 SIMD**(6절): A(NEON, 추천) 또는 B(스칼라). 1단계의 앱 측정 뒤 정한다.
2. **Mac 하드웨어**: Apple Silicon만(추천). Apple은 macOS 26을 Intel Mac의 마지막 판으로 밝혔고, Intel까지 하면 SIMD·digest 조합이 하나 더 는다.
3. **macOS 서명**: 유료 Apple Developer 계정(Developer ID 서명·공증)을 쓸지. 업데이트와 첫 실행 안내가 이것에 달렸다.
4. **Linux 범위**: x64 AppImage, X11·Wayland 둘 다(제안). 배포판 패키지(deb·rpm·Flatpak)는 범위 밖으로 두고 나중에 본다.
5. **지원 OS 하한**: macOS 몇 판부터, Linux glibc 몇부터. 1단계 전에 정한다.
