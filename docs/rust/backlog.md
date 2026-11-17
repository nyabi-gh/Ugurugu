# 3.x 백로그

2026-10-12 정리, 2026-11-17 갱신. 3.x(Rust, `apps/ugurugu`, `crates/*`)에 아직 열려 있는 일을 모은다. M0~M6 계획과 검토 보고서를 지우기 전에 거기서 뽑았다. 지운 문서는 git `70f9454`에 있다(`git show 70f9454:docs/rust/m6-plan.md` 등). 출처 칸의 줄 번호는 그 커밋 기준이다. 어디 칸은 줄 번호가 아니라 함수 이름으로 가리킨다(줄 번호는 코드가 바뀌면 어긋난다).

- C++ 2.x와 웹 판은 더 유지하지 않는다(사용자 결정). 2.x 전용 항목과 2.x 릴리스는 버렸다. 2.2.13 소스는 `v2.2.13` 태그에만 있고(2026-10-13 저장소에서 지움), 비교 기준과 `.ugu` fixture 생성기가 필요하면 그 태그를 빌드한다.
- 각 항목은 지금 코드에서 싸게 확인할 수 있는 것은 확인했다. 확인하지 않은 영향·원인은 **가설**로 적는다.
- ID `B3-nn`은 이 문서 전용이다. 원래 문서의 ID(U3-xx, A-xx, B-xx 등)는 출처 칸에만 쓴다.
- 항목은 종류별로 묶는다. 어디서 나왔는지는 출처 칸에 남긴다.

## 출처 칸의 이름

| 출처 | 문서 |
|---|---|
| m6-plan, m5-plan, m4-plan, m0-close, m0-evidence | 마일스톤 계획과 옛 측정 기록, `70f9454` |
| 분석 | `docs/analysis-3.0.md`(`b809009` 기준), `70f9454`. A-01~A-05는 모두 고쳤다(m6-plan 6절) |
| 검토 | `docs/RUST_3_REVIEW_2026-10-12.md`(`03ef8a8` 기준), `70f9454`. U3-01~U3-12는 모두 고쳤다(m6-plan 6절) |
| ANALYSIS_REPORT | C++ 2.2.x 분석 `docs/ANALYSIS_REPORT.md`, `70f9454`. Rust 코드에서 같은 문제를 확인한 것만 옮겼다. 해당하지 않는 것: B-01(위가 클리핑 레이어면 병합 거절, `command.rs` `merge_down`), B-06(접촉 중 barrel은 획을 이어 감, `input.rs`), B-07(펜이 떠나도 키 상태 유지), U-02(`Ctrl+Shift+Z` 있음), U-03(파일 연결·여러 창), R-01(이미지 형식 모두 디코드), B-14(500단계·256MiB). R-02는 B3-12에 합쳤고, R-08(읽기 퍼징)은 3.3.0에서 닫았다(evidence.md 65절) |
| plan-document-tabs | `docs/rust/plan-document-tabs.md`, `70c27f4` |
| 3.2.0 계획 | `docs/rust/plan-animation.md`, `d0506ba`. 정한 것은 decisions.md 4절 "키프레임과 애니메이션 폴더"와 ADR 8절에 있다 |
| 2026-10-13 코드 검토 | 3.0.0 출시 뒤 영역별 코드 검토(읽기만, 일부는 시험 crate로 재현). 고친 것은 3.0.1에 들어갔다 |
| 3.1.0 출시 전 검토 | 2026-10-14 외부 검토 보고서(`8195424` 기준, 저장소에 두지 않음). 발견 R01~R10을 코드로 확인했고, 재현은 보고서 작성자가 공용 crate를 직접 부른 시험 crate로 했다 |
| 3.1.0 종합 검토 | 2026-10-14 3.1.0(`f96d785`) 종합 검토 보고서(저장소에 두지 않음) |
| 2026-10-21 UI 검토 | 헤드리스로 그린 화면 65장과 UI 코드로 본 검토(evidence.md 88절). 고친 것은 PR #117(decisions.md 9절) |
| 2026-11-14 #187 검토 | 외부 기여 PR #187(Linux 3단계)을 병합한 뒤 `crates/ugu-platform/src/linux/`와 앱 쪽 변경을 읽은 검토. 빌드하거나 Linux에서 실행하지 않았다. "확인"은 코드 경로를 끝까지 읽었다는 뜻이다. 코드로 확인된 것은 2026-11-16에 고쳤다(evidence.md 127절) |

## 1. 사용자 할 일

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-142 | 간체 중국어 번역을 읽어 본다 | 2.2.13 번역이 없어 새로 옮겼고 중국어를 읽는 사람의 검토를 받지 않았다(decisions.md 9절). 용어와 어색한 문장을 고친다 | `apps/ugurugu/i18n/strings.txt`의 다섯째 열 | 2026-10-16 간체 중국어 |
| B3-176 | 실제 펜으로 페이드 획을 그어 보고, 외곽선을 켠 레이어에도 그어 본다 | Sandbox의 합성 마우스로 모양과 픽셀 연필의 칸 숨김을 확인했고, 켠 경로의 프레임 시간도 쟀다(evidence.md 110·111절). 합성 입력에는 압력이 없어, 압력이 굵기와 곱해진 획은 실행 앱에서 보지 않았다. 3.6.0은 이 확인 없이 냈다 | 브러시의 페이드 칸, 레이어 속성의 외곽선, `crates/ugu-render/src/stroke.rs` | 2026-11-08 이슈 #157 |
| B3-177 | 굵은 브러시를 실제 펜으로 그어 본다 | 크기 상한을 512px로 올렸고(decisions.md 3절), 그리는 중에 샘플마다 다시 그리던 것을 프레임마다 한 번 그리게(evidence.md 121·122절), 펜을 뗄 때 dab 획을 여러 스레드로 칠하게 고쳤다(123절). 모두 마우스 입력(시험, `ugu-doc`, Sandbox의 1000Hz 끌기)으로만 쟀고, 3.6.2는 이 확인 없이 냈다. 할 일: 압력이 있는 펜으로 512px 잉크 펜과 에어브러시를 4096² 캔버스에 그어, 포인터를 따라가는지 보고 펜 떼기를 앱 로그(`pen-up shown`)로 읽는다. 이슈 #157이 바란 750px은 형식의 획 굵기 상한(512)을 올려야 해 하지 않았다 | `apps/ugurugu/src/canvas.rs` `show_stroke`·`end_drawing`, `crates/ugu-render/src/dab.rs` `Painter::paint` | 2026-11-12 이슈 #157 |
| B3-96 | 채우기 기준 그림과 대기 변형 | 영역 클릭은 이제 대기 변형을 먼저 적용하고 기준 그림을 읽는다. 대기 글자도 같이 적용되므로(`session.settle`), 배치 중인 글자를 둔 채 자동 선택을 쓰면 글자가 확정된다. 의도와 맞는지 사용자 확인 | `apps/ugurugu/src/canvas.rs` `click_area` | 2026-10-13 코드 검토 |

## 2. 열린 gate

출시를 막지 않는다. 통과로 적지 않고, 릴리스 노트에 시험한 범위만 쓴다.

| ID | gate | 지금 근거 | 닫는 방법 | 출처 |
|---|---|---|---|---|
| B3-06 | 실제 펜·터치 장치 | Windows Sandbox 합성 입력(`InjectSyntheticPointerInput` PT_PEN·PT_TOUCH): 압력·barrel·hover·창 밖 해제·1000개/초, 두 손가락 팬·핀치·회전. 지우개 끝은 Windows가 합성 플래그를 넘기지 않아 단위 시험만(`canvas.rs` `a_pens_eraser_end_erases_whatever_tool_is_chosen`). Linux는 KDE Plasma Wayland에서 실제 펜 태블릿의 그리기·필압을 확인했고(evidence.md 125절), 지우개 끝·barrel과 X11 `XInput2`는 단위 시험만 있다 | Windows는 태블릿 드라이버에서 **Windows Ink를 켜고**(WinTab은 읽지 않는다), Linux는 Wayland의 지우개 끝·barrel·hover 커서(`wp_cursor_shape_v1`)·창 밖 해제와 X11 세션의 압력·지우개·barrel·근접 마우스 억제, 터치 두 손가락 제스처와 펜 우선을 확인한다. 같이 볼 가설: Windows에서 `frameId`·`PerformanceCount`를 0으로 보내는 장치는 샘플 순서가 뒤집힐 수 있다. Shift+누름 직선은 누른 첫 표본의 압력이라 펜으로는 가늘 수 있고, barrel을 누른 접촉은 오른쪽 버튼으로 와서 Alt와 함께면 브러시 크기를 바꿀 것이다(decisions.md 3절, 둘 다 가설). 닫으면 릴리스 노트의 펜 문장을 고친다 | m6-plan 2절 10, 6절 M6-9·M6-10, 7절, 8절, 2026-11-14 Linux 3단계 |
| B3-07 | RTX 5080·WARP 밖의 GPU | Windows PC의 RTX 5080(DX12)·`UGURUGU_ADAPTER=warp`, macOS의 Apple M4(Metal), Linux 헤드리스의 AMD Radeon RX 7900 XTX(`RADV NAVI31`, Vulkan)와 Mesa `llvmpipe`(`UGURUGU_ADAPTER=fallback`). 하드웨어 표면 실패 → 소프트웨어 대체 자동 전환은 Linux `Xvfb`·`WLR_RENDERER=pixman`에서 `RADV` → `llvmpipe`로 확인했고(evidence.md 125절), Windows에서는 Sandbox가 호스트 GPU를 보여 줘서 만들지 못했다. Intel iGPU와 Linux 실제 데스크톱 창의 여러 드라이버(NVIDIA 독점·ANV 등)는 보지 않았다 | 사용자 보고(로그의 `selected GPU adapter`, 정보 탭의 진단 정보 복사). 다른 PC에서 실행하거나 B3-10의 진단 옵션을 만든 뒤 iGPU로 실행 | m6-plan 7절, 8절, 분석 A-03·6절, 2026-11-14 Linux 3단계 |

## 3. macOS·Linux 실제 장치 확인

3.6.3부터 macOS(3.6.4부터 서명·공증한 dmg)와 Linux(x64 AppImage)도 낸다(evidence.md 130절). 아래는 코드는 있지만 실제 기계·세션·장치에서 보지 않은 것이다. 펜·GPU 전반은 2절의 gate에 있다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-13 | macOS·Linux 3.x 실제 장치 확인 | ① 2단계의 펜과 최근 문서는 코드가 있다(decisions.md 3·7절의 2026-11-16 행, evidence.md 128절). 화면과 실제 펜으로 보지 않은 것: 필압·지우개 끝·hover, barrel 버튼(태블릿 설정이 오른쪽 누름으로 바꿀 때 `buttonMask`에도 보이는지는 **가설**), 펜을 태블릿에 놓아 둔 채 쓰는 마우스, monitor를 단 뒤의 마우스·트랙패드, 병합을 끈 뒤 표본이 모두 오는지(**가설**), Dock 메뉴의 최근 문서. ② 낸 뒤에야 보는 것: GitHub에서 받아 quarantine이 붙은 zip을 "그래도 열기"로 열고 그 번들이 업데이트되는지, Linux AppImage의 업데이트, x64 AppImage를 실제 X11·Wayland 데스크톱에서 띄우기. AppImage는 `.ugurugu` 파일 연결을 하지 않는다(`tools/linux_desktop.py`는 실행 파일 경로를 적는다). Linux 실행 파일에 `line-tables-only` 디버그 정보가 남는다(arm64 시험 빌드에서 223MB, 그 AppImage 57MB, evidence.md 129절): macOS·Windows처럼 따로 떼는지 정한다. ③ `Occluded` 변경 전후의 최소화 중 CPU. ④ 끄는 중 Option+Esc가 크기를 되돌리는지(Windows Sandbox의 합성 입력에서는 Alt를 쥔 채 누른 Esc가 창에 오지 않았다, evidence.md 107절. **가설**: macOS에서는 키가 앱에 와서 되돌린다). ⑤ 메뉴 막대를 화면으로 보지 않았다: 항목 누르기, 저장 패널 이름 칸의 ⌘C·⌘V, 입력 칸의 ⌘C, 가리기·최소화 키(evidence.md 118절은 메뉴 내용과 실행 경로를 앱 안에서 봤다). ⑥ 한글·일본어 IME를 macOS에서 확인한 적 없다(evidence.md 27절은 Windows다). ⑦ 실제 로그아웃·재시동으로 종료 경로를 보지 않았다(evidence.md 117절은 앱 안에서 보낸 Apple Event다). ⑧ 한글 파일 이름: `NSURL`은 경로를 자소로 분해해 돌려준다(2026-11-09 시험). 탭 이름 등에 그대로 쓰면 글자가 풀어져 보일 수 있다(**가설**, 화면으로 보지 않았다). ⑨ 인터페이스 글꼴(간체 중국어의 macOS Hiragino Sans GB, Linux `fc-match`)이 없을 때 안내가 없고 없는 글자는 네모로 보인다. ⑩ 배포 대상은 macOS 14(decisions.md 10절)이지만 macOS 14 기계에서 실행해 보지 않았다 | `crates/ugu-platform/src/macos/`, `.github/workflows/`, `apps/ugurugu/src/main.rs`, `apps/ugurugu/src/theme.rs` `chinese_font_file`, `apps/ugurugu/src/ui/actions.rs` `take_pressed` | m6-plan 2절 14, 분석 3.1·4절 14, 2026-11-09 재개 검토 |
| B3-178 | Linux 펜 근접과 마우스 억제 | 펜이 놓여 있는 동안 마우스가 막히던 것, 누른 채 빠진 X11 장치, 조용해진 X11 펜의 `Leave`는 고쳤다(decisions.md 3절, evidence.md 127절). 남은 것: ① X11에서 winit의 연결과 태블릿 스레드의 연결 사이에 순서가 없어, 펜 누름이 마우스 누름으로 먼저 처리될 수 있다(구조는 확인, 얼마나 자주인지는 **가설**). 근접이 오면 마우스가 쥔 버튼을 놓으므로(evidence.md 126절) 그 획이 점 하나로 끝나고 펜이 이어 그릴 것이다(**가설**, 실행해 보지 않음). ② X11에서 펜을 곧게 들어 올리면 다음 입력이 올 때까지 egui의 hover와 브러시 커서가 마지막 자리에 남는다. X의 포인터도 그 자리에 남는다. 150ms 뒤에 스스로 지우면 가만히 든 펜의 커서가 깜박일 것이라(**가설**) 그대로 두었다. ③ `wp_cursor_shape_v1`이 없는 compositor에서는 숨긴 펜 커서를 되돌리지 않는다(코드는 확인, 화면은 **가설**). ④ 고친 규칙을 실제 Wayland·X11 세션과 펜으로 보지 않았다: 놓아 둔 펜 옆에서 마우스가 먹는지, 펜과 마우스를 번갈아 쓸 때 두 번 그려지지 않는지 | `crates/ugu-platform/src/linux/wayland_tablet.rs` `PenQueue`·`apply_tool_cursor`, `x11_tablet.rs`, `apps/ugurugu/src/winit_pointer.rs` `translate` | 2026-11-14 #187 검토 |
| B3-179 | Linux 클립보드 | 읽기 기한을 마지막 바이트부터 세게, 쓰기 스레드가 읽지 않는 상대를 3초 뒤 놓게, 입력 직렬 번호가 없을 때의 쓰기를 오류로 고쳤다(evidence.md 127절). 남은 것: ① Wayland 쓰기는 `latest_serial`이 낡아 compositor가 `set_selection`을 무시해도 성공으로 답한다(**가설**, compositor마다 다르다). 그때 소스 넷 상한(`active_sources.remove(0)`)이 실제 선택을 없앨 수 있다. ② X11 쓰기는 `arboard::Clipboard`를 만들고 곧 버린다. 선택이 남는 것은 egui-winit이 창이 도는 동안 같은 arboard 3.6.1 클립보드를 쥐고 있어서다(arboard의 `Drop`과 egui-winit `clipboard.rs`를 읽음). ③ 붙여넣을 때마다 시스템 클립보드의 PNG를 읽어 해시한다(큰 캔버스에서 느릴 것이라는 **가설**). 복사가 실패하면 그때도 한 번 읽는다 | `crates/ugu-platform/src/linux/clipboard.rs` `write_image`, `linux/window.rs` `handle_command`, `apps/ugurugu/src/clipboard.rs` | 2026-11-14 #187 검토 |
| B3-180 | Linux 단일 실행과 문서 넘기기 | 문서를 넘기지 못한 둘째 실행은 이제 실패 코드로 끝나고, 듣는 쪽은 느린 연결을 2초에 끊으며 창 닫기가 그 연결을 기다리지 않는다. 시험은 자기 폴더를 쓴다(decisions.md 7절, evidence.md 127절). 남은 것: 넘기지 못한 문서는 여전히 열리지 않는다. 데스크톱에서 파일을 두 번 눌러 시작한 실행은 표준 오류를 볼 사람이 없다. 까닭 셋 가운데 첫 실행이 5초 넘게 바쁜 경우는 요청이 대기열에 남아 나중에 열릴 것이고(**가설**, 실행해 보지 않음), 두 실행의 runtime 폴더가 다르거나 쓸 수 없는 경우는 첫 실행에 닿을 길이 없다 | `crates/ugu-platform/src/linux/instance.rs` `hand_over`, `apps/ugurugu/src/main.rs` | 2026-11-14 #187 검토 |
| B3-182 | Linux 세션 끝 | 취소된 종료 뒤 inhibitor를 다시 잡고 복구를 다시 쓰게, 세션 끝 스레드가 어떻게 끝나든 `ACTIVE_LISTENERS`가 내려가게 고쳤고, 시험은 logind의 잠금을 잡지 않는다(decisions.md 7절, evidence.md 127절). 남은 것: ① 로그아웃만 할 때는 SIGTERM이 표시 연결이 끊기기 전에 오는지에 달렸다(**가설**, GNOME·KDE에서 보지 않음). ② 고친 logind 경로(`PrepareForShutdown`의 참·거짓, 다시 잡는 inhibitor)를 실제 logind로 보지 않았다 | `crates/ugu-platform/src/linux/session.rs` `watch_logind_shutdown` | 2026-11-14 #187 검토 |
| B3-183 | Linux 포털 대화상자 | `Response`를 포털이 보낸 것만 받게, 포털이 사라지면 오류로 끝나게, 포털이 필터를 알려 주지 않으면 이름의 확장자로 형식을 정하게, 폴더 열기가 `xdg-open`을 기다려 실패를 로그에 남기게 고쳤다(decisions.md 9절, evidence.md 127절). 남은 것: 어느 것도 실제 포털로 보지 않았다. 포털을 죽였을 때 대화상자 뒤의 열기·저장이 다시 되는지, bus가 well-known 이름의 `sender` 규칙을 포털의 unique 이름으로 맞추는지(D-Bus 규격은 그렇게 적는다)를 GNOME·KDE에서 본다 | `crates/ugu-platform/src/linux/dialog.rs` `show_once`, `folder.rs` `show` | 2026-11-14 #187 검토 |
| B3-184 | Wayland 작업 스레드 | 표시 연결을 읽거나 이벤트를 처리하다 오류가 나면 스레드가 끝나게 고쳤다(evidence.md 127절, 판정 함수의 시험만 있고 끊긴 연결로 돌려 보지 않았다). 남은 것: 창을 만들 때 UI 스레드가 roundtrip을 1초까지 기다린다. 메모리 예산은 cgroup 한도를 보지 않는다 | `crates/ugu-platform/src/linux/window.rs` `run_wayland_worker`, `memory.rs` | 2026-11-14 #187 검토 |

## 4. 출시 뒤로 미룬 결정

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-08 | M6-3 시작 어댑터 나열 약 0.3초 | wgpu `request_adapter`가 어댑터 4개의 드라이버를 모두 연다(어댑터마다 첫 D3D12 장치 110~170ms). wgpu 30에는 하나만 여는 공개 API가 없다(`dx12::Adapter::expose` 비공개). fork·패치는 하지 않기로 했다. 창이 뜬 뒤라 체감은 첫 프레임 약 0.48초의 일부. wgpu를 올릴 때 다시 본다 | `crates/ugu-render/src/gpu.rs` | m6-plan 2절 3, 3절, M6-3, m5-plan M5-2 |
| B3-09 | M6-12 2.2.13 ③ 채우기 클릭→화면 측정 공백 | 2.2.13 값을 재지 못했다(측정 도구 scratchpad `fill_probe.py`, 사용자 PC 사용으로 값 버림). 2.x를 더 유지하지 않으므로 비교 기준으로만 의미가 있다. 버려도 된다 | GUI 측정 | m6-plan 3절, 6절 M6-12 |
| B3-10 | Intel iGPU 등 어댑터를 고르는 진단 옵션 | 지금 `UGURUGU_ADAPTER`는 소프트웨어 대체(`fallback`·`warp`·`software`)만 안다. 특정 하드웨어 어댑터(iGPU 등)를 이름·전력 선호로 고르려면 더 필요하다 | `crates/ugu-render/src/gpu.rs` `from_env` | m6-plan 4절 순서, 7절, 2026-11-14 Linux 3단계 |
| B3-11 | 델타 업데이트 | 2.2.13도 `--delta None`. 전체 패키지 15.5MB | `tools/package_windows.ps1` | m6-plan 1절 |
| B3-12 | 코드 서명 | 서명 없이 내기로 했다(2026-10-11 사용자). SmartScreen 경고는 도움말에 있다. 서명하게 되면 `vpk pack --signParams`만 더한다. macOS는 3.6.4부터 Developer ID로 서명·공증한다(decisions.md 10절). 2.x 분석 R-02의 지적도 그대로 해당한다: Windows 업데이트의 신뢰 근거가 GitHub 릴리스 하나뿐이라 계정·토큰이 털리면 업데이트 경로로 코드가 나간다(서명 전까지는 2FA·토큰 범위 관리) | `tools/package_windows.ps1`, `release-3.yml` | m6-plan 2절 7, 5절; ANALYSIS_REPORT R-02 |
| B3-14 | macOS 2.2.13의 업데이트 확인 | 2.2.13의 Sparkle 피드는 `releases/latest/download/appcast.xml`이다. 3.x가 GitHub Latest라 이 주소는 404다(2026-10-13 `curl`로 확인, `v3.0.1`로 리디렉션된 뒤 404). m6-plan은 "업데이트 없음"으로 끝난다고 적었지만 2.2.13에서 확인하지 않았다(**가설**). Sparkle이 404를 오류로 보여 줄 수도 있다. 자동 확인이면 조용히 실패하고 수동 확인이면 오류 창이 뜰 것으로 보인다(가설). 피하려면 3.x 릴리스마다 2.2.13의 `appcast.xml`을 자산으로 함께 올리는 방법이 있다(정하지 않음) | GitHub Releases, `release-3.yml` | m6-plan 2절 14 |
| B3-15 | ARM64 Windows | 범위 밖으로 뺐다. 빌드 대상·libwebp·ring·Velopack 런타임을 함께 봐야 한다 | `.cargo/config.toml`, 패키지 | m6-plan 1절, 8절 |
| B3-78 | 문서 탭을 창 밖으로 떼어 새 창 만들기 | 3.0 탭은 순서 바꾸기만 한다(사용자 결정 2026-10-12). 떼려면 문서를 다른 프로세스로 넘겨야 한다 | `apps/ugurugu/src/ui/tabs.rs`, `canvas.rs` `move_tab`, `render.rs` | [plan-document-tabs.md](https://github.com/nyabi-gh/Ugurugu/blob/70c27f4/docs/rust/plan-document-tabs.md) 3절 3 |
| B3-80 | 탭이 많을 때 보이는 탭의 표면 예산 | 숨은 탭은 레이어 표면을 놓지 않고(⑤ 탭당 약 405MiB), 표면 예산은 탭마다 빈 메모리로 정한다. 메모리가 적은 PC에서 큰 문서 탭이 많으면 보이는 탭이 바닥값(128MiB)으로 내려가 레이어를 하나씩 그릴 수 있다(가설, 8GiB PC에서 재지 않음). 고치려면 숨은 탭 작업자에 표면을 비우는 일을 보내고 예산을 창 전체에서 보이는 탭 먼저 나눈다. 탭마다 래스터 스레드 풀(8개)을 따로 두는 것도 함께 볼 수 있다(쉬는 스레드라 비용은 측정상 없음) | `apps/ugurugu/src/cache.rs`, `budget.rs`, `canvas.rs` | evidence.md 41절, plan-document-tabs 4절 5 |
| B3-82 | 일본어 줄바꿈 금칙 | egui는 글자 단위로 줄을 바꾸므로 작은 가나·장음·구두점이 줄 머리에 올 수 있다(설명 글 몇 곳). 넘치지는 않는다 | egui 줄바꿈 | evidence.md 43절 |
| B3-83 | 날짜 표기의 언어 | 복구 안내 등의 날짜는 시스템 로캘로 쓴다(Windows `GetDateFormatEx`, macOS `NSDateFormatter`). UI 언어가 다르면 섞인다(한국어 Windows + 일본어 UI에서 "오전"). Linux는 `YYYY-MM-DD HH:MM` 고정이라 섞이지 않는다 | `crates/ugu-win/src/clock.rs`·`crates/ugu-platform/src/macos/clock.rs` `local_text` | evidence.md 43절 |
| B3-79 | egui 기본 모양이 남은 곳 | 드롭다운의 닫힌 버튼(`egui::ComboBox`)은 egui 것을 팔레트로 칠해 쓴다(높이 28·모서리·색은 테마 값, 화살표는 egui 삼각형). 다른 위젯과 어긋나는지는 사용자가 Screen set 화면으로 본다. 선 속성의 색 버튼은 `dc666c1`에서 앱의 색 견본(`widgets::color_swatch`)으로 바꿈 | `ComboBox` 쓰는 곳 | 2026-10-12 위젯 통일, evidence.md 42절 |

### 의존성과 상류를 기다리는 것

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-117 | Cargo 의존성 갱신 주기 | Dependabot은 GitHub Actions만 매달 본다. Cargo 의존성은 정해진 검토 주기가 없다. 자동 major 갱신보다 호환 묶음을 native 목록·회귀 화소·성능 기준선과 함께 확인하는 주기를 정한다. B3-44·B3-45·B3-72·B3-185를 그때 같이 본다 | `.github/dependabot.yml`, `Cargo.toml` | 3.1.0 종합 검토 9.3절 |
| B3-33 | egui-wgpu 장치 손실 panic 우회 제거 | egui-wgpu가 손실된 장치에서 오류 대신 panic하므로 장치 손실 때만 `catch_unwind`로 흡수한다. 상류가 오류를 돌려주게 고쳐지면 지운다 | `apps/ugurugu/src/render.rs` `RenderThread::frame`의 `catch_unwind` | m0-close 58행 |
| B3-34 | egui TextEdit의 UIA `SetValue` | egui가 지원하지 않아 일부 보조 기술이 제한된다. 수용했고 egui를 올릴 때 다시 본다 | egui | m0-close 64행 |
| B3-45 | egui-winit을 올릴 때 `EguiState` 수동 `Send` 재검토 | "AccessKit adapter가 비어 있으면 옮겨도 된다"는 가정에 기댄다. 지금 판에서 문제를 재현한 것은 아니다 | `apps/ugurugu/src/render.rs` `EguiState`의 `unsafe impl Send` | 분석 3.2; 검토 9절 |
| B3-44 | `derivative` 대체 추적 | Velopack이 끌고 오는 빌드 시점 매크로로, RUSTSEC-2024-0388(관리 중단 공지, 취약점 아님)을 `deny.toml`에서 허용한다. Velopack 쪽 교체를 지켜본다. Velopack main은 2026-10-16에 뺐고, 그것이 든 판은 프리릴리스 1.3.39뿐이다(B3-145) | `deny.toml`의 `[advisories] ignore`, `Cargo.lock` | 분석 3.5; m6-plan 6절 M6-5 |
| B3-145 | Velopack의 zip을 9로 올리기 | ugu-io는 2026-10-29에 zip 9.0.5로 올렸다(decisions.md 11절). Velopack 1.2.161이 zip `^8.6`을 요구해 zip 8.6.0이 함께 남고, 그 판의 RUSTSEC-2026-0367·0368·0369·0429(9.0.0에서 고침)를 `deny.toml`에서 허용한다. 넷 다 `ZipArchive::extract`·`ZipStreamReader::extract` 안의 결함인데 Velopack 1.2.161은 `by_index`로 읽어 직접 파일에 쓴다(2026-10-17 소스로 확인, 2026-11-05 crates.io의 1.2.161로 다시 봄: `extract`·`ZipStreamReader`·`is_symlink`를 부르지 않는다). 8.6.0의 Dependabot 경고도 같은 까닭으로 해당하지 않아, 2026-11-05 사용자 결정으로 다섯 건 모두 "vulnerable code not used"로 닫았다: GHSA-m665-v7pm-7grj(경고 4), `extract`의 symlink인 GHSA-q7v9-x93c-xqv3(경고 5, 0429와 같음)·GHSA-hjqx-j4rw-g7xv(경고 7). 경고 6(GHSA-8hxf-qr8p-62x4, `aes-crypto`)과 경고 8(GHSA-9hj8-73fh-9963, `legacy-zip`)은 Velopack이 켜지 않는 기능 안이다: zip을 `default-features = false`에 `deflate`만으로 받고, `Cargo.lock`의 zip 8.6.0에는 `aes`·`hmac`·`bitstream-io`가 없다. zip 9를 받는 Velopack은 2026-10-30 현재 1.3.39-g8d2844f(`derivative`·`syn` 1이 빠지고 Unix에 `nix`가 듦)뿐인데, GitHub에서 프리릴리스로 표시돼 있고 그 판의 `Update.exe`·`Setup.exe`는 zip 9.0.4로 빌드됐다(태그의 `Cargo.lock`). 패키지 전체를 푸는 것은 이 두 실행 파일이라, 1.3.39로 올리면 우리 `Cargo.lock`의 경고만 닫히고 설치·업데이트 경로는 여전히 9.0.4다. 그래서 올리지 않았다(2026-10-30). 닫는 조건: 프리릴리스가 아닌 Velopack 판이면서 태그의 `Cargo.lock`이 zip 9.0.5 이상(Velopack main은 2026-10-30에 9.0.5). 그때 crate와 `ci.yml`·`release-3.yml`의 `vpk`를 함께 올리고, Windows 패키지 job의 설치·실행·제거와 이전 판에서의 업데이트를 확인한 뒤 허용 네 줄을 지운다 | `Cargo.toml` `velopack`, `deny.toml` | 2026-10-17 `cargo deny`, 2026-10-29 Dependabot 경고 4, 2026-11-02 Advisories(RUSTSEC-2026-0429)와 경고 5~8 |
| B3-185 | Linux에서만 드는 관리 중단 crate 넷 | `deny.toml`의 `targets`가 Linux와 macOS도 보게 된 뒤(#187) `cargo deny --locked check advisories`가 여섯 건으로 실패했다. `rustix` 1.1.5(RUSTSEC-2026-0486)와 yank된 `async-executor` 1.14.0은 1.1.6과 1.14.1로 올렸다. 남은 넷은 취약점이 아닌 관리 중단 공지라 이유를 적어 허용한다. ① RUSTSEC-2026-0370·0371·0372(`zvariant`·`zbus_names`·`zvariant_derive`): zbus 6에 합쳐졌다. AccessKit(`accesskit_unix` 0.21.1, egui-winit 0.36.2가 정한다)이 zbus `5.5`를 요구하므로(그 crate의 `Cargo.toml`), `ugu-platform`의 zbus만 6으로 올리면 zbus가 둘이 되고 셋은 그대로 남는다. 닫는 조건: egui를 올려 AccessKit이 zbus 6을 쓸 때 함께 올리고 허용 세 줄을 지운다(B3-117). ② RUSTSEC-2026-0192(`ttf-parser` 0.25.1): winit의 `wayland-csd-adwaita`가 `sctk-adwaita` 0.10.1 → `ab_glyph`로 끌어와, compositor가 창 테두리를 주지 않는 곳(GNOME)에서 제목 글자를 그린다. winit 0.30.13의 다른 선택은 제목 없는 테두리(`wayland-csd-adwaita-notitle`)와 FreeType을 쓰는 `wayland-csd-adwaita-crossfont`다. 닫는 조건: `sctk-adwaita`가 다른 글꼴 파서로 옮기거나, 제목 없는 테두리를 받아들이기로 정한다. Windows와 macOS의 앱에는 이 crate들이 없다 | `deny.toml`의 `[advisories] ignore`, `Cargo.toml` `zbus`, `apps/ugurugu/Cargo.toml`의 winit 기능 | 2026-11-14 #187 검토 뒤의 검사 |

## 5. 알려진 한계

기록된 차이다. 고칠 계획은 없고, 필요하면 개선 후보로 옮긴다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-16 | 변형 상자 밖(회전 자리)의 커서가 화살표 | 회전 손잡이는 egui `Alias`인데 Windows에서 winit이 화살표로 바꾼다. 2.2.13에는 회전 손잡이가 없어 비교 대상이 없다 | `apps/ugurugu/src/ui/mod.rs`(`Grip::Rotate`의 `CursorIcon::Alias`), `render.rs` | m6-plan 7절 GUI 확인, 8절 |
| B3-17 | 파일을 열 때 내장 이미지를 끝까지 디코드해 검증 | 4096² 한 장에 최대 약 50ms(압축 잘 되는 그림 47ms, 잡음 18ms) 더 걸린다. 손상 PNG를 정상처럼 여는 것(A-01)을 막는 비용 | `crates/ugu-io/src/read.rs` `draws_as` | m6-plan 6절 분석 보고서 확인, 8절 |
| B3-18 | RDP는 Windows Sandbox 원격 세션까지만 확인 | Sandbox도 `TerminalServerSession`이지만 `mstsc`로 다른 PC에 접속한 실행과 같다고 보는 것은 가설 | — | m6-plan 7절 |
| B3-19 | 업데이트 경로 중 시험하지 않은 것 | GitHub `updates` 릴리스에서 받는 실제 업데이트(`HttpSource`)는 Windows 3.0.0 → 3.0.1에서 확인했다(evidence.md 46절). 설치 뒤 적용 실패(`update-install-failed`)는 만들어 내지 못했다. 여러 창이 떠 있을 때 갱신기가 다른 창을 어떻게 닫는지, 바이러스 백신이 있는 실제 PC도 보지 않았다. macOS·Linux는 B3-13 ② | `apps/ugurugu/src/update.rs` | m6-plan 6절 M6-5·M6-7 결과, 7절, 8절 |
| B3-20 | 그리는 중 픽셀 획은 프레임마다 전체를 다시 계산 | 3초 빠른 획 끝에서 약 1.5ms/프레임(잉크 펜의 10배 이상). 프레임을 놓치지 않아 그대로 둔다. B3-24와 관련 | `crates/ugu-render/src/live.rs` `LiveStroke::update_pixels`, `pixel.rs` | m6-plan 6절 M6-16, 8절 |
| B3-21 | 진행 중 dab 획은 표본 상한(`MAX_DABS` 5만)을 모르고 그린다 | 진행 중 획은 상한 없는 `Resampler::new`를 쓰고, 확정 경로(`stroke_dabs`)만 `Resampler::at_most(…, MAX_DABS)`다(2026-11-17 코드로 확인). 그래서 상한을 넘는 긴 에어브러시·스프레이 획은 확정 뒤 간격이 넓어진 모양으로 바뀔 것이다(앱에서 다시 보지 않았다) | `crates/ugu-render/src/live.rs`, `dab.rs` `stroke_dabs` | m4-plan 603행 |
| B3-22 | Windows 10은 따로 시험하지 않음 | 막지 않고 "Windows 10 이상"을 둔다(사용자 결정). Sandbox 게스트는 Windows 11 계열(10.0.26100) | — | m6-plan 2절 12 |
| B3-143 | 번체 중국어 시스템은 영어(또는 다음 선호 언어)로 보임 | 중국어 번역은 간체 하나다(사용자 결정 2026-10-16). 번체 표시(`Hant`·`TW`·`HK`·`MO`)가 있으면 `catalogue`가 번역이 없다고 답해 다음 선호 언어, 없으면 영어로 간다. 번체 번역을 더하려면 문자열 열과 글꼴(번체 자형)을 함께 정해야 한다 | `apps/ugurugu/src/i18n.rs` `catalogue` | 2026-10-16 간체 중국어 |
| B3-60 | 임시 파일을 만들 수 없는 폴더에는 저장할 수 없음 | 저장은 같은 폴더의 `.<이름>.<pid>-<n>.tmp`를 거쳐 교체한다. 직접 쓰기 fallback이 없다. 그런 폴더(ACL·일부 공유)가 실제로 문제인지는 가설이고, fallback은 원자성을 잃으므로 하려면 사용자에게 묻는 편이 맞다 | `crates/ugu-io/src/save.rs` `temporary_path` | ANALYSIS_REPORT B-25 |
| B3-124 | 움직인 레이어의 브러시 커서 크기와 큰 선택 끌기 | 브러시 커서는 레이어 크기 키를 곱하지 않고 문서 크기로 보인다. 움직인 레이어에서 큰 선택을 끄는 동안은 화면 해상도 미리보기(`Glance`) 대신 원본으로 그린다 | `apps/ugurugu/src/canvas/cursor.rs`, `canvas/transform.rs` `refresh_preview` | 3.2.0 구현 |

## 6. 개선 후보

### 결함·견고성

코드로 확인했거나 코드로 보아 생길 수 있는 결함이다. 재현하지 않은 것은 **가설**로 적는다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-86 | 8192px보다 넓은 창 | wgpu 기본 한도(`max_texture_dimension_2d` 8192)로 장치를 연다. 여러 모니터를 하나로 묶은 넓은 화면에서 최대화하면 swap chain 설정이 실패하고 그려지지 않는다(가설, 그런 화면 없음). 어댑터 한도를 요청하고 크기를 한도로 자른다 | `crates/ugu-render/src/gpu.rs`, `present.rs` | 2026-10-13 코드 검토 |
| B3-87 | 2.x 가져오기의 clip 마스크 훑기 | 가져오기는 서로 다른 (선택 마스크, `visibilityClip`, 캔버스)마다 그 사각형을 한 번 훑는다. 연산 20,000개가 모두 다른 큰 사각형이면 훑는 화소가 수천억에 이르러 파일 스레드가 오래 막힐 수 있다(가설, 재지 않음). 게다가 clip이 빈 획(선택 마스크가 사각형 안에서 모두 128 미만인 경우 등)은 훑은 뒤 `Importer::op`이 `None`을 돌려줘 연산으로 세지 않는다(`paint`의 `self.operations += 1`은 `Some(op)`일 때만 돈다, 2026-10-16 코드로 확인). 그래서 20,000 상한이 훑는 횟수를 막지 못하고 그런 획의 수는 파일 길이(`FILE_BYTES` 128MiB)로만 제한된다(가설, 재지 않음). 만든 mask 바이트·연산 수·점 수·이미지는 쌓일 때 한도를 본다. 안: 마스크마다 128 이상인 화소의 경계를 한 번만 구해 두고 사각형과 겹치는지 먼저 보거나, 훑은 화소 수에 상한을 둔다 | `crates/ugu-io/src/legacy.rs` `masked`, `bits` | 2026-10-13 코드 검토 |
| B3-93 | 선택 윤곽 정점 버퍼 한도 | 윤곽 run이 약 1340만 개를 넘으면 `max_buffer_size`(256MiB)를 넘어 그 프레임 전체가 무효가 된다. 4096² 잡음 위 자동 선택 같은 극단만 | `crates/ugu-render/src/ants.rs` | 2026-10-13 코드 검토 |
| B3-95 | 같은 레이어가 같은 획 id를 두 번 쓸 때 재스타일 | 선택이 한쪽만 건드리면 `RemoveStroke`가 남은 참조 때문에 실패해 오류가 난다. 그런 상태로 가는 순서는 찾지 못했다(가설) | `crates/ugu-core/src/restyle.rs` | 2026-10-13 코드 검토 |
| B3-127 | 키가 있는 레이어의 이미지 크기 바꾸기 | `resample_image`는 키의 이동값과 중심을 축마다 비율로 늘린다. 가로세로 비율이 다르게 바뀌면 회전한 키는 늘어난 화소와 어긋나고(자세는 한 가지 크기와 회전만 가짐), 늘린 값이 `limits::OFFSET`(32,767)을 넘으면 문서 검사에 걸려 크기 바꾸기 전체가 거절된다. 2026-10-14 재검토에서 코드로 확인했고 앱에서 재현하지는 않았다 | `crates/ugu-core/src/command.rs` `resample_image`, `rekeyed` | 3.2.0 재검토 |
| B3-131 | 못 읽은 복구 폴더의 30일과 시계 | `failed-<유닉스 시각>-…`의 나이는 이름의 시각만 믿는다. 옮길 때 시계가 과거였다가 고쳐지면 다음 시작이 바로 지운다(가설, 재현하지 않음). 파일 시스템 시각도 같은 시계라 그것으로는 고치지 못한다 | `apps/ugurugu/src/recovery.rs` `failed_long_ago` | 3.1.0 출시 전 검토 |
| B3-132 | render 스레드가 창 스레드를 기다리는 남은 길 | 세션이 끝나는 동안 창 스레드는 복구 쓰기를 4초 기다린다. 그때 render 스레드가 장치 손실 뒤 새 표면을 청하고 있으면(`UiEvent::NeedSurface`, 창 스레드만 만든다) 둘 다 4초를 쓴다. 창을 주인으로 하는 대화상자가 그 순간 열리는 경우도 주인 창을 동기 메시지로 끄는지는 보지 않았다(가설). 제목과 포인터는 2026-10-15에 창 스레드로 옮겼다 | `apps/ugurugu/src/app.rs` `ending`과 `surface_source` 클로저, `render.rs` `SurfaceSource` | 3.1.0 출시 전 검토 |
| B3-169 | 바깥에서 모든 창에 보낸 `WM_CLOSE`가 winit의 이벤트 창도 없앤다 | winit 0.30.13의 사용자 이벤트 창(`Winit Thread Event Target`)은 보이는(`WS_VISIBLE`) 투명 popup이고 `WM_CLOSE`를 `DefWindowProcW`에 넘겨 스스로 없어진다. 그 뒤 render 스레드가 보내는 사용자 이벤트는 닿지 않는다(evidence.md 104절에서 Sandbox 시험이 그 창을 닫았을 때 첫 surface 요청이 오지 않았다). `/F` 없는 `taskkill`처럼 프로세스의 모든 최상위 창에 `WM_CLOSE`를 보내는 도구로 끄면, 앱 창은 닫기를 받아도 render 스레드가 끝났다는 알림(`RenderStopped`)이 오지 않아 프로세스가 남을 것이다(가설, 재지 않음). 확인한 뒤 고치는 방법(상류 winit, 또는 앱이 그 창의 `WM_CLOSE`를 받지 않게)을 정한다 | `apps/ugurugu/src/app.rs`, winit `platform_impl/windows/event_loop.rs` | 2026-11-05 Windows PC 확인 |
| B3-114 | 사용자 강조색에 따라 포커스 선·버튼 글자의 대비가 무너짐 | `ink_on`은 패널 휘도가 0.2 미만이면 대비를 보지 않고 그만두고, `accent_text`는 선형화하지 않은 RGB 가중합의 임계값(0.52, 2.2.13)으로 글자 색을 고른다. `theme.rs` 계산을 Python으로 옮겨 잰 값: 어두운 패널과 같은 `#24262B`이면 선/패널 1.000:1(포커스 선이 보이지 않음), `#808080`이면 글자/강조 3.786:1, 기본 `#FFC94A`는 어두운 테마 9.880:1·글자 11.572:1, 밝은 테마 선 3.036:1. 앱 화면으로 본 값은 아니다. 고치려면 고른 색은 두고 배경마다 밝게·어둡게 파생하고, 글자는 두 후보의 실제 대비로 고른다. 닫는 조건: 검정·흰색·중간 회색·패널과 같은 색·고채도 색을 두 테마에서 글자 4.5:1, 선 3:1로 시험 | `apps/ugurugu/src/theme.rs` `ink_on`·`accent_text`, `widgets.rs` `focus_ring` | 3.1.0 종합 검토 N01 |
| B3-168 | 상태 표시줄의 포인터 위치가 UI Automation에 없음 | 상태 표시줄의 `x … y …` 글(`ui/mod.rs` `view_controls`의 `egui::Label`)이 Sandbox의 UI Automation 트리에 나오지 않았다(2026-10-26, 같은 창에서 "Layer 1", "Frame 1 of 30"은 나옴). 화면 읽기 프로그램도 그 위치를 읽지 못할 것이다(가설). egui가 어떤 글을 AccessKit에 내는지부터 본다 | `apps/ugurugu/src/ui/mod.rs` `view_controls` | evidence.md 96절 |

### 기능·UI

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-186 | 그라데이션 도구·디더·필터 레이어·1비트 패턴 | NewfolderGames가 포크 브랜치 `feat/dithering`(2026-11-17, `095f946` 위 커밋 4개, 69파일 +7,027줄)에서 만든 PoC. 본인이 PoC로만 두겠다고 했다: 패턴 하나 때문에 브러시·채우기·획 형식·칠하기 함수가 모두 바뀌었고, 커스텀 패턴·특수 효과의 기반을 노렸다. 읽은 것: 기존 문서의 regression digest는 그대로, 새 기능 이름(`filter-layer`·`gradient-op`·`pattern`)은 옛 판이 거절하는 기존 규칙대로, 그라데이션 보간·스크린톤 각도는 고정소수점. 문제: 오차 확산(Floyd-Steinberg·Atkinson) 필터는 그쪽 측정(Ryzen 9 5900X, 8스레드, 2048² 16레이어)에서 합성 14→135ms, pen-up 1.4→136~140ms다(아래 편집마다 전체 캔버스를 직렬로 다시 계산). 방사형 그라데이션이 `f64::hypot`(libm마다 다를 수 있다, **가설**)을 쓴다. 넣는다면 작게 나눈다: 필터 레이어 뼈대와 위치 기반 효과 하나 → 패턴을 필터 레이어의 한 종류로(획 형식을 바꾸지 않음, 획마다 다른 패턴은 레이어를 나눠야 한다) → 그라데이션 도구 → 오차 확산은 느린 비용을 어떻게 다룰지 정한 뒤. 빌드·시험은 돌려 보지 않았다 | https://github.com/NewfolderGames/Ugurugu/tree/feat/dithering | 2026-11-17 사용자 요청 분석 |
| B3-23 | 설정 "그리는 동안에도 우글거림 유지" | 2.2.13에 있던 선택지. 3.0은 2.2.13 기본값(획 동안 멈추고 끝나면 다시 재생)만 옮겼다. 원하면 넣는다 | `apps/ugurugu/src/canvas.rs` `pause_playback`, 설정 그리기 탭 | m6-plan 2절 22, 8절 |
| B3-30 | 패널을 메인 창 밖(두 번째 모니터)으로 떼기 | 자체 도킹은 떠 있는 패널도 메인 창 안이다. 2.2.13 Qt 도크와 남은 차이 | `apps/ugurugu/src/layout.rs`, `ui/dock.rs` | m5-plan 119행(M5-11) |
| B3-52 | 작업 흐름 안내 | 도움말에는 "시작하기"가 있지만 첫 실행에 띄우지 않는다. 문자 안내(`text-hint`)는 확정하면 윤곽이 되어 다시 편집할 수 없다는 말이 없다. 삽입한 이미지를 캔버스에 맞춰 줄여도 "삽입됨"만 알린다(`import::decode`의 `fit`). 메뉴의 "현재 프레임을 이미지로 내보내기"와 동작 이름 "이미지로 내보내기"가 다르다. 움직이는 GIF·WebP를 삽입하면 첫 프레임만 가져오지만(`import.rs`) 그렇다고 알리지 않는다. 확인 창을 늘리기보다 결과가 바뀌는 자리(문자 패널, 실제로 줄였을 때의 알림)에 짧게 둔다. 문구 예: "적용하면 글자가 선으로 바뀝니다", "이미지를 6000×4000에서 1024×683으로 줄여 넣었습니다"(숫자는 실제 값) | `apps/ugurugu/src/ui/`, `files.rs`, `i18n/strings.txt`, `crates/ugu-io/src/import.rs` | 검토 6절 "작업 흐름 개선 제안", 3.1.0 종합 검토 |
| B3-53 | PNG 연속 프레임·sprite sheet 내보내기 | 게임 엔진·영상 편집으로 재사용, 픽셀 모드와 잘 맞는다 | `apps/ugurugu/src/export.rs` | 분석 3.8 |
| B3-54 | 폴더 밖 레이어의 어니언 스킨 | 3.2.0의 어니언 스킨은 애니메이션 폴더와 그 셀에서만 켜진다(`Canvas::onion_wanted`는 `session.folder()`가 없으면 `None`). 보통 레이어에서 앞뒤 프레임을 겹쳐 우글거림 진폭·끊김·연결을 정지 상태에서 비교하는 것은 아직 없다. `RenderPlan::alone`은 레이어 id 하나를 그 프레임의 자세로 그리므로 폴더가 아닌 레이어에도 쓸 수 있어 보이나 확인하지 않았다(가설). 키프레임 편집기 전체가 아니라 포즈 비교로 한정 | `apps/ugurugu/src/canvas/onion.rs` `onion_wanted`, `crates/ugu-render/src/plan.rs` `RenderPlan::alone` | 분석 3.8, decisions.md 4절 |
| B3-55 | 모션 seed 다시 뽑기·잠금, A/B 비교 | 같은 설정에서 다른 느낌을 찾고 고른 결과를 재현 | `crates/ugu-core/src/store.rs`, `apps/ugurugu/src/ui/mod.rs` | 분석 3.8 |
| B3-136 | 내보내기 미리보기와 프리셋 | 애니메이션 창은 크기·인코딩 메모리·재생 길이·GIF 가장자리 안내를 보인다(3.3.0). 마지막 크기와 픽셀 확대 배수는 기억한다(`settings.rs` `export_edge`·`pixel_export_times`). 최종 인코딩과 같은 옵션으로 바꾼 한 프레임 미리보기와, 이름 있는 몇 개의 내보내기 프리셋은 없다 | `apps/ugurugu/src/files.rs` 애니메이션 창, `export.rs` | 분석 3.8, 3.1.0 종합 검토 |
| B3-62 | 고대비·글자 크기 설정 없음 | 테마는 어둡게·밝게·시스템 설정과 같게 중에서 고르지만(decisions.md 9절) Windows 고대비 색은 따르지 않는다. 본문 12·작은 글자 11포인트 고정. 시험용 `UGURUGU_UI_SCALE`(egui zoom, decisions.md 7절)이 이미 있어 UI 크기를 설정으로 내기는 쉽다(2026-10-21 UI 검토) | `apps/ugurugu/src/theme.rs`, `app.rs` | ANALYSIS_REPORT U-08; 검토 6절 |
| B3-63 | 필압 곡선·최소값과 입력 장치 상태 표시가 없음 | 필압은 켬·끔(`tools.tablet.pressure`)뿐이다. 필압이 먹지 않을 때 UI에서 펜 감지·압력 지원 여부를 볼 수 없다(로그·진단 정보는 있음). B3-06 실기기 확인 때 함께 본다 | `apps/ugurugu/src/settings/tools.rs`, `crates/ugu-win/src/pointer.rs`, `apps/ugurugu/src/winit_pointer.rs` | ANALYSIS_REPORT U-09 |
| B3-120 | 캔버스 밖 여백 | 레이어 표면이 캔버스 크기라 레이어를 옮기면 가장자리가 비어 들어온다. 캔버스 밖에서 들어오는 움직임에는 표면에 여백이 있어야 하고, 타일 계산·예산이 바뀐다 | `crates/ugu-render/src/document.rs` `reach` | 3.2.0 계획 4.5 |
| B3-133 | 프레임마다 움직이는 레이어의 병합 | 한 자리에 둔 레이어는 이동을 구워 병합한다(decisions.md 4절). 자세가 바뀌는 레이어는 구간이 자기 키를 가져야 같은 모습이 되어 형식이 바뀐다. 키가 같은 두 레이어라도 각각 옮겨 합성한 것과 합성해 옮긴 것은 가장자리 화소가 다르고, 불투명도 키는 레이어마다 따로 곱한다 | `crates/ugu-core/src/ops.rs` `merge_down` | 3.2.0 계획 9절 |
| B3-134 | 곡선 경로·베지어 보간 편집 | 키 사이는 일정·부드럽게·멈춤만 있고 이동은 직선이다. 경로를 곡선으로 그리고 손잡이로 보간 곡선을 고치는 UI와 형식이 필요하다 | `crates/ugu-core/src/keys.rs` `pose` | 3.2.0 계획 9절 |
| B3-135 | 셀 복사로 노출 채우기 | 셀을 복사해 여러 프레임에 노출하는 편집(예: 같은 셀을 다음 빈 프레임들에 잇기). 무엇을 채울지(빈 프레임만, 범위) 정해야 한다 | `crates/ugu-session/src/animating.rs` | 3.2.0 계획 9절 |
| B3-140 | 대칭자의 남은 것 | 대칭자(decisions.md 4절)는 파일에 저장하지 않고, 도형 도구는 따르지 않는다. 후보: 파일 저장(형식에 선택 필드), 도형과 채운 도형의 복사, 복사가 같이 우글거리는 선택지. 큰 획이 많으면 펜을 뗄 때가 길다: 2048²에서 문서 대부분을 덮는 획 24개는 약 40ms이고, 그 절반 가까이가 획 경계 사각형마다 coverage를 읽어 놓는 일이다(evidence.md 68절) | `crates/ugu-session/src/symmetry.rs`, `apps/ugurugu/src/canvas/symmetry.rs`, `crates/ugu-render/src/compose.rs` `stamp_strokes` | 2026-10-15 대칭자 |
| B3-159 | 강조색이 많은 곳에 칠해짐 | 모든 슬라이더의 채운 부분(`theme.rs`의 `slider_trailing_fill`과 `selection.bg_fill`)과 우글거림을 켠 레이어 줄의 아이콘이 강조색(기본 앰버)이라 decisions.md 9절의 "강조색 채움은 지금 켜진 것 하나" 원칙과 어긋나고 현재 도구 표시가 약해진다. 경고·거절 안내도 같은 강조색으로 상태 표시줄 왼쪽 아래에 나와(`ui/mod.rs` `status_bar`) 펜 근처에서 거절당한 사용자가 놓치기 쉽다(가설, 사용자 시험 없음) | `apps/ugurugu/src/theme.rs`, `ui/layers.rs`, `ui/mod.rs` | 2026-10-21 UI 검토 |
| B3-160 | 새 문서 크기 버튼이 픽셀 그림용뿐 | 16·32·64·128만 있다(`files.rs` `ask_new_document`). 512², 1080², 1920×1080처럼 움직이는 그림에 흔한 크기가 없다 | `apps/ugurugu/src/files.rs` | 2026-10-21 UI 검토 |
| B3-161 | 작은 안내·표시 | 저장 확인(`unsaved-text`)이 어느 문서인지 이름을 말하지 않는다(창 닫기는 탭을 차례로 보여 주며 묻는다). 단축키 페이지가 88개(2026-11-17)를 메뉴별 묶음 없이 한 목록(줄 약 52pt)으로 보인다. 도구 설정 머리가 늘 "도구 설정"이라 지금 도구 이름이 없다(도구 막대는 아이콘만). 색 칸의 `#AARRGGBB`(2.2.13)는 웹·디자인 도구가 `#RRGGBBAA`로 읽어 복사하면 다른 색이 된다 | `apps/ugurugu/i18n/strings.txt`, `ui/settings.rs`, `ui/dock.rs`, `ui/color.rs` | 2026-10-21 UI 검토 |

### 성능

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-24 | 픽셀 획을 Vello 없이 레이어 픽셀에 직접 쓰기 | 너비 12 픽셀 획은 1스레드 1.5배·8스레드 1.4배(계단 경계를 Vello가 채우는 비용). 확정 스탬프는 이미 직접 쓴다. 렌더 단계 순서와 축소 렌더를 다시 짜야 한다 | `crates/ugu-render/src/pixel.rs`, `document.rs` | m5-plan 115행(M5-16), m0-evidence 966행 |
| B3-25 | 픽셀 칸이 그대로인 프레임은 다시 그리지 않기 | 재생은 첫 바퀴 뒤 프레임을 모두 쥐므로 첫 바퀴에만 이득 | `crates/ugu-render` | m5-plan 115행 |
| B3-26 | 큰 타원 선택 회전에서 출력 행마다 구간 모으기 | 32 → 20ms까지 줄였고 더 줄일 여지 | `crates/ugu-core` 선택 마스크 | m5-plan 117행(M5-12) |
| B3-27 | 선택 끌기 미리보기를 만드는 비용 | 누를 때 62~69ms(새 픽스맵 8장 할당과 평균), 갱신은 2048² 합성 5ms라 160Hz(6.25ms)에 못 미친다. 후보: 같은 대기 변형 안에서 다시 끌 때 재사용, 남김·뺌을 한 번에 평균, 원본 부분을 남긴 것에서 잘라 쓰기 | `crates/ugu-render/src/moving.rs` `Split::begin_move`·`glance`, `apps/ugurugu/src/canvas/transform.rs` | m5-plan 117행, m0-evidence 880행 |
| B3-28 | 획 윤곽을 묶음으로 흘려 보내기 | 1스레드 렌더가 레이어 전체 윤곽을 쥔다(③ 39, ④ 59MiB). 윤곽 만들기와 그리기 사이 동기화가 늘어난다 | `crates/ugu-render` | m5-plan 118행(M5-13) |
| B3-29 | 큰 텍스처 업로드를 프레임 인코더에 합치기 | 지금은 큰 업로드마다 따로 submit·poll해 render 스레드가 0.25ms 더 쓴다 | `crates/ugu-render/src/view.rs` | m5-plan 118행 |
| B3-31 | 축소 내보내기를 작게 그린 뒤 줄이기 | 작은 크기에서는 시간·메모리 대부분이 원래 크기 그리기다. 다만 2.2.13 NativeExact(원래 크기로 그린 뒤 면적 평균)와 화소가 달라져 하지 않았다. 차이를 받아들일 때만 | `apps/ugurugu/src/export.rs` | m5-plan 120행 |
| B3-32 | 100% 이상으로 큰 문서를 재생할 때 보이는 영역만 그리기 | 프레임이 화면 크기로 줄어 예산이 모자란 PC의 재생이 빨라진다 | `apps/ugurugu/src/cache.rs` | m4-plan 652행 |
| B3-36 | 내보내기의 정적 레이어 재사용 | 내보내기 렌더는 revision에 `None`을 넘겨(`render_plan(..., None, ...)`) 정적 레이어를 다시 쓰지 않는다. 복잡한 정적 배경의 긴 애니메이션은 매 프레임 다시 그린다(비용 크기는 가설). 문서 식별자와 revision을 함께 넘겨야 하고 `None == None`으로 재사용하면 다른 문서가 섞인다 | `apps/ugurugu/src/cache.rs` `run`의 `Job::Export`, `crates/ugu-render/src/document.rs` | 분석 3.3-3, 4절 11 |
| B3-40 | 타일이 사용 영역의 경계 사각형 버퍼 | 두 모서리에 작은 표시만 있어도 큰 사각형이 필요하고, 커질 때 이전·새 버퍼가 겹친다. sparse tile map이 아니다(비용은 가설) | `crates/ugu-render/src/tile.rs` | 분석 3.3-2 |
| B3-50 | 긴 텍스트의 윤곽을 UI 스레드에서 다시 만듦 | 배치한 텍스트가 바뀌면 render/UI 스레드에서 Parley 윤곽을 다시 만든다(같은 글이면 캐시). 긴 문장·대량 붙여넣기는 입력 지연 후보. 재고 길이 제한·지연 합치기·백그라운드 배치를 검토 | `apps/ugurugu/src/canvas/text.rs` `Typesetter::outline` | 검토 8절 |

### 코드·빌드·시험

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-35 | Pedantic Clippy 정리 | workspace 정책은 `clippy::all = warn`. pedantic 경고 블록 1,749개(`cast_possible_truncation` 433 등). 좌표·바이트·픽셀 변환의 범위 검증부터 | `Cargo.toml` `[workspace.lints]` | 분석 3.4, 4절 13, 6절 |
| B3-37 | 2.2.13 fixture → 3.0 전체 프레임 비교를 CI에 | 지금 CI는 가져오기 대응·거절만 시험하고(`legacy/tests.rs`), 2.2.13 렌더와의 프레임 비교(`ugu-doc compare`)는 수동이었다(m0-evidence 22절). C++를 더 빌드하지 않으므로 2.2.13 생성기로 기준 프레임 PNG를 한 번 만들어 저장소에 두고 CI가 비교하는 방식이 맞다 | `tests/fixtures/ugu13`, `tools/ugu-doc`, `.github/workflows/ci.yml` | 분석 2.2 |
| B3-47 | 코드에 직접 넣은 사용자 문구 검사 | Fluent 키 일치 시험은 언어 파일끼리만 비교해 코드의 하드코딩 영어를 찾지 못한다. 지금 남은 하드코딩 문구가 있는지는 전수 확인하지 않았다(가설) | `apps/ugurugu/src/i18n.rs` | 분석 3.7; 검토 6절 |
| B3-51 | 축소 재생 미리보기의 픽셀 모드 fixture | 재생 때 `preview_shrink`는 해상도·표본 간격을 줄인다. 곡선의 축소·표본 간격 비교는 `document.rs` 시험 두 개가 한다(`a_smaller_render_is_the_full_render_made_smaller`, `fewer_samples_keep_a_smaller_frame_within_the_same_limits`). 픽셀 모드의 축소 미리보기 비교 fixture는 없다 | `apps/ugurugu/src/canvas.rs`, `crates/ugu-render/src/document.rs` | 검토 8절, 10절 |
| B3-67 | 릴리스 PDB를 보관하지 않음 | Release는 `debug = "line-tables-only"`로 PDB를 만들지만 Windows 패키지는 exe·LICENSE·고지 세 파일만 담고 `release-3.yml`도 PDB나 다른 기호 파일을 올리지 않는다(macOS·Linux 패키지도 같다). 네이티브 충돌 기록(모듈+오프셋)을 기호로 풀 수 없고, panic backtrace에도 함수 이름이 빠질 것으로 보인다(가설) | `Cargo.toml` `[profile.release]`, `tools/package_windows.ps1`, `.github/workflows/release-3.yml` | ANALYSIS_REPORT R-11 |
| B3-68 | 릴리스를 초안 없이 만든 뒤 자산을 올림 | `softprops/action-gh-release`가 곧바로 공개 릴리스를 만들고 파일을 올린다. 그 사이 `releases/latest/download/…` 같은 링크가 자산 없는 릴리스를 볼 수 있다(창의 길이·영향은 가설). 앱의 업데이트 피드는 해당하지 않는다: 3.0.1부터 고정 사전 릴리스 `updates`에서 읽고 그 패키지는 이 단계보다 먼저 올라간다(decisions.md 10절). `draft: true`로 만들고 올린 뒤 공개 | `.github/workflows/release-3.yml`의 `softprops/action-gh-release` 단계 | ANALYSIS_REPORT R-12 |
| B3-118 | `files.rs`의 상태 전이 나누기 | `files.rs`(3,597행, 2026-11-17, 시험 일부는 `files/` 하위 모듈로 나뉨)에 파일 작업자, 저장 상태, 복구, 대화상자, 내보내기 질문이 모인다. 파일만 쪼개면 같은 가변 상태를 공유해 이해 비용이 줄지 않으므로, "질문 대기 → 특정 탭의 편집 확정 → 저장 요청 → 특정 상태의 응답 → 닫기·업데이트" 전이를 먼저 그리고 대화상자 표시·작업자 메시지·탭의 저장·복구 상태를 나눈다. 탭 id·저장 당시 상태·요청 id가 만나는 곳에는 타입 구분을 더한다. 닫는 조건: 늦은 저장 응답, 질문 중 탭 전환, 닫기 취소, 같은 경로로 저장, 내보내기 중 업데이트가 전과 같이 동작 | `apps/ugurugu/src/files.rs`, `canvas.rs` | 검토 4절, 3.1.0 종합 검토 6절 |
| B3-130 | Sandbox 둥근 픽셀 점 단계의 확대율 | "Round pixel dots at several zooms"가 확대율 칸에 100을 넣었는데 500%로 찍혔다(evidence 60). 단계는 이제 확대율을 읽어 결과에 적지만(`$shown`) 다르면 실패하지 않고, 획을 긋기 전에 넣는 100은 읽지 않는다. 왜 100이 들어가지 않았는지 찾고, 찍기 전에 확대율을 확인해 다르면 실패하게 한다 | `tools/sandbox/gui.ps1` | evidence 60, 70 |
| B3-170 | pointer 시험의 합성 입력이 가끔 오류 87로 실패하고, 실패한 단계가 다음 단계를 오염시킨다 | 2026-11-05 `pointer.ps1`의 "Many samples at once"가 34번째 표본에서 `InjectSyntheticPointerInput` 오류 87(ERROR_INVALID_PARAMETER)로 멈췄다. 원인은 모른다(다시 돌린 두 번은 2,000표본 모두 들어갔다). 그 단계는 예외로 빠지면서 펜을 떼지 않고 `$script:before`도 갱신하지 않아, 그때까지 그린 획을 다음 "One finger does not draw"가 새 획으로 세어 함께 실패했다(evidence.md 104절). 실패한 단계도 펜을 떼고 획 수를 다시 세어 다음 단계가 깨끗이 시작하게 한다. 오류 87 때 표본 번호·위치·경과 시간은 이미 남긴다(`c0c496a`). 다시 나오면 원인을 가를 수 있게 그 표본의 플래그와 앞 표본과의 시간 차도 남긴다 | `tools/sandbox/pointer.ps1` `Send`, "Many samples at once" | 2026-11-05 Windows PC 확인 |

## 7. 측정·확인 공백

재지 못했거나 원인을 나누지 못한 것, 생길지 모르는 문제를 재현하지 않은 것. 근거는 `evidence.md`의 해당 주제.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-69 | WARP 지연의 원인을 나누지 못함 | WARP에서 입력이 60~75ms 늦게 보이고 단계 20~32%가 합쳐진다. 렌더 시간인지 합성 대기인지 나누지 않았다 | `crates/ugu-render/src/gpu.rs` | evidence.md 장치 손실·TDR·WARP |
| B3-70 | ② 8 작업자에서 30fps 미달 | 8스레드 프레임 ② 42~49ms, ②p 35.4~35.6ms(evidence.md 13·14절). 사용자가 받아들였지만 목표는 열려 있다(decisions.md 1절). 다음 수단은 인코드·래스터 일 줄이기(가설) | `crates/ugu-render/src/document.rs` | evidence.md 작업자 수와 레이어 배분 |
| B3-71 | CPU 제조사가 다를 때 픽셀 동일성 | Windows Intel(UCRT, 104절), macOS Apple M4(FMA를 뺀 NEON, macOS `libm`, 101·115절), Linux AMD Ryzen 9 5900X(Zen 3 SSE4.2, glibc 2.44 `libm`, 125절) 세 조합에서 fixture ①~⑤의 생성 바이트와 8스레드 렌더 digest가 모두 같았다. 남은 가설: `f64` `sin`·`cos`·`hypot`은 다른 `libm` 판이나 FMA3 분기에 따라 1ulp 달라질 수 있다(`dab.rs` 스프레이, `stroke.rs`, `ugu-core`의 `motion.rs`). 한 PC 안의 미리보기·최종 동일성과는 무관 | `crates/ugu-render`, `crates/ugu-core/src/motion.rs` | evidence.md 렌더러·101·104·115·125절 |
| B3-72 | `vello_cpu`·`vello_common` 0.x API 위험 | 0.x라서 판을 올릴 때 API가 바뀔 수 있다. 판 고정과 회귀 시험으로 막고 있다. 펜을 뗄 때 찍는 선은 Vello의 strip 생성기를 직접 돌리고 coarse 단계의 채우기 규칙을 따라 하므로(decisions.md 2절), 판을 올리면 `compose::tests::an_aliased_stroke_stamped_lays_the_pixels_a_full_render_draws`가 전체 렌더와 바이트가 같은지 본다 | `Cargo.toml`, `crates/ugu-render/src/compose.rs` `Stamp::line_coverage` | evidence.md 렌더러·92절 |
| B3-73 | 떠 있는 패널의 Tab 순서 | 떠 있는 `egui::Window` 패널 사이의 키보드 이동 순서를 확인하지 않았다 | `apps/ugurugu/src/ui` | evidence.md IME·글꼴·접근성·키보드 |
| B3-76 | 시작 직후 제목의 `*` | 2026-10-12 사용자 실행에서 새 문서 제목이 `제목 없음*`로 떴다. 다음 실행에서는 없었다. 원인 미확인(가설: 복구 또는 픽셀 모드 상태) | `apps/ugurugu/src/files.rs` | 2026-10-12 사용자 실행 |
| B3-38 | 프로세스 전체 메모리 예산과 저메모리·여러 창 측정 | 캐시 `Budget`은 최소 256MiB를 강제하고(가용 100MiB여도), 디코더·히스토리·GPU staging·내보내기·다른 창은 이 예약에 묶이지 않는다. 이미지 디코더 1GiB 한도도 고정값이다. 저메모리·여러 창·큰 내보내기 조합의 OOM·지연은 재현하지 않았다(가설). 잴 조건: 8·16GiB, 큰 문서 탭 1·4·8개, 저장·가져오기·내보내기 조합에서 peak private bytes, commit 여유, 탭 바꾸기 지연, 취소 p95. B3-80과 같이 예산을 한 곳에서 나누고 숨은 탭부터 회수하는 정책을 정하되, 예산 초과 split의 화소 일치 시험(`over_the_budget_the_split_shows_the_bytes_of_a_full_render`)을 지킨 채로 한다 | `apps/ugurugu/src/budget.rs` `FLOOR`, `crates/ugu-io/src/import.rs` `ALLOCATION` | 분석 3.3-1·3.3-5, 4절 12; 검토 8절 |
| B3-39 | 내보내기 대기 상한·공정성 | 캐시 작업자는 편집·재생을 내보내기보다 먼저 하고 내보내기는 `answer.recv()`로 기한 없이 기다린다. 계속 편집할 때 내보내기 완료·취소 정리가 얼마나 늦는지 재야 하고, 늦으면 시간 할당이나 대기 상한을 둔다. 편집 우선 자체는 결함이 아니다. 정상 작업을 실패로 만드는 시간 초과보다, 취소가 대기 중 요청·진행 중 렌더·인코더·파일 정리까지 닿는지와 내보내기의 최소 진행 기회를 먼저 본다. 잴 것: 연속 편집·재생 중 내보내기 완료 시간, 취소→정리 p95(목표 100ms) | `apps/ugurugu/src/cache.rs` `Renders::render`(`answer.recv()`)와 `Queue`의 `exports` | 분석 3.3-4, 4절 11; 검토 8절 |
| B3-92 | 펜 위치의 HIMETRIC 환산 | 펜·터치 위치를 `ptHimetricLocation`과 장치 사각형으로 환산하고 `ptPixelLocation`과 비교하지 않는다. 태블릿 영역 매핑·비율 유지·세로 화면에서 어긋날 수 있다(가설). B3-06 실기기 확인 때 본다 | `crates/ugu-win/src/pointer.rs` | 2026-10-13 코드 검토 |
| B3-97 | 막 뜬 창이 잠깐 UI Automation에 답하지 않음 | Sandbox 시험에서 앱을 띄운 직후 `AutomationElement.FromHandle`이 HRESULT 0x80131505(COR_E_TIMEOUT)로 끝나는 일이 되풀이됐다(evidence.md 45절, 53절, 60절). 시험 스크립트가 곧바로 실패한 것은 Windows PowerShell 5.1이 이것을 `TimeoutException`이 아닌 `COMException`으로 받아서였다(77절, 시험 쪽은 고침). 이제 시험은 다시 묻고, 그렇게 통과한 단계는 "pass after retry"와 무엇을 몇 번 다시 물었는지를 남긴다. 창은 보이지만 그 순간 창 스레드가 메시지를 돌리지 않는 것으로 보인다(가설, 원인 미확인). 화면 읽기 프로그램은 시작 직후 잠깐 창을 읽지 못할 수 있다. "pass after retry"가 어느 단계에서 얼마나 자주 나오는지 모아 원인을 가른다 | `apps/ugurugu/src/app.rs` 시작 순서 | evidence.md 45절, 60절, 67절, 77절 |
| B3-125 | 움직인 레이어에서 그릴 때의 입력→표시 | headless pen-up만 쟀다(evidence.md 55절). Sandbox는 합성 입력이라 앱의 입력→표시 지연(latency-probe)은 사용자 PC에서 잰다 | `tools/latency-probe` | 3.2.0 계획 7절 |
| B3-162 | Windows 정밀 터치패드의 두 손가락 스크롤 | Windows의 휠 입력(`ugu-win` `PointerEvent::Wheel`)은 캔버스 위에서 세로는 늘 확대·축소이고 가로는 캔버스가 받지 않는다(`input.rs`). 그래서 Windows 정밀 터치패드의 두 손가락 끌기가 이동이 아니라 확대·축소가 될 것이다(가설, 장치 없음). macOS·Linux에서 winit이 픽셀 단위로 주는 스크롤(`PixelDelta`)은 캔버스를 옮긴다(`5d81eba`, macOS 트랙패드) | `apps/ugurugu/src/input.rs`, `crates/ugu-win/src/pointer.rs`, `apps/ugurugu/src/winit_pointer.rs` | 2026-10-21 UI 검토 |
| B3-165 | Sandbox에서 그은 선에 창 오른쪽 아래로 뻗는 선 수십 개 | 2026-10-22 "Key opacity, pivot and onion settings" 재실행 한 번에서, `Draw-Stroke`의 한 획이 원래 선과 함께 선 위 여러 점에서 창 오른쪽 아래 구석 쪽으로 뻗는 선 수십 개를 그렸다(어두운 픽셀 10899, 같은 단계의 다른 세 번은 445 안팎). `Find-Paper`가 획 직전에 포인터를 창 오른쪽 아래에 둔다. 바로 앞 실행은 앱이 15초 안에 UI Automation에 답하지 않아 실패했으니 VM이 느렸다. 가설: 느린 VM에서 `SetCursorPos`로 옮긴 옛 위치와 `mouse_event` 절대 이동이 섞여 들어왔다. 실제 마우스·펜에서도 생기는지는 모른다. 다시 보이면 그 실행의 포인터 입력 로그(`UGURUGU_LOG` debug)로 샘플 위치를 본다 | `tools/sandbox/gui.ps1` `Find-Paper`·`Draw-Stroke`, `apps/ugurugu/src/input.rs` | 2026-10-22 |
| B3-166 | 3.4.1~3.4.3 검토에서 확인하지 못한 것 | 2026-10-22 서브 에이전트 검토(렌더링·도크·메뉴와 키·설정)가 코드로만 짚었고 재거나 재현하지 않은 것. ① AltGr는 Windows에서 Ctrl+Alt로 오고 egui-winit은 모르는 논리 키(€, ę)를 물리 키로 바꾸므로(`logical_key.or(physical_key)`, 소스로 확인), 독일어·폴란드어 배열에서 캔버스 위 AltGr+E가 GIF 내보내기(Ctrl+Alt+E)를 연다. 글자 칸 안에서는 단축키를 받지 않는다. ② 우글거림 그룹은 높이를 정하기 전에는 다 보일 높이가 최소라서, 부드럽게·끊어진 선으로 행이 늘면 오른쪽 영역이 경계 없는 스크롤로 바뀌어 높이를 줄일 길이 없을 수 있다(가설, `dock.rs` `least_of`). ③ 메모리 예산을 넘은 split이 편집 레이어 위 레이어를 하나씩 따로 들어(`stream.rs`) 예산이 막으려던 만큼 쓸 수 있다(측정 없음). ④ 채운 도형을 끄는 동안 움직일 때마다 캔버스 크기 선택을 만든다(`ugu-session` `shaping.rs` `shape_fill`, 측정 없음). ⑤ 포즈 칸을 고친 뒤 바로 이동 도구로 끌면 칸의 `changed()`가 `edit`를 거쳐 막 시작한 포즈 끌기를 끝낼 수 있다(프레임 안 순서 미확인, `ui/motion.rs`). ⑥ 설정 저장이 계속 실패하면 2초마다 경고 한 줄이라 9시간쯤이면 로그 앞부분이 밀려난다. ⑦ 사용자가 Alt+S·L·A를 다른 동작에 줬으면 새 메뉴가 Alt 글자로 안 열린다(F·E·V·T·W·H도 같았다) | `apps/ugurugu/src/shortcuts.rs`, `ui/dock.rs`, `crates/ugu-render/src/stream.rs`, `crates/ugu-session/src/shaping.rs`, `ui/motion.rs`, `settings.rs` | 2026-10-22 검토 |
