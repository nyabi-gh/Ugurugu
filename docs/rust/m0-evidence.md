# M0 측정·실증 기록

환경은 [scope.md](scope.md) 1절(주 모니터 UHD 60Hz 150%, RTX 5080, 마우스만). 측정일 2026-10-06.

## 1. 입력→화면 표시 지연

도구: `tools/latency-probe` + PresentMon 2.6.0(Intel 서명 확인, 관리자 권한). 두 앱을 같은 도구, 같은 절차로 잰다.

1. 도구가 PresentMon(`--no_track_input`)을 먼저 시작한 뒤 앱을 실행한다.
2. 창을 2560×1600 외곽 크기(client 2538×1549)로 맞춘다.
3. 주 버튼을 누른 채 캔버스 중앙의 나선(반경 = client 짧은 변 × 0.12) 획을 `SendInput`으로 한 칸씩 늘린다. 칸 사이에는 15~40ms 무작위 간격을 두어 vsync 위상과 상관을 없앤다.
4. 칸마다 다음 입력 전의 첫 present를 응답으로 본다. 지연은 `SendInput` 직전 QPC에서 그 present의 `TimeInQPC + MsUntilDisplayed`까지다.

이 값은 scanout 시작까지이며 패널 응답 시간은 포함하지 않는다. "입력 뒤 첫 present = 응답"이 성립하려면 앱이 유휴 상태에서 present하지 않아야 한다. 두 앱 모두 유휴 5초 동안 present 0회였다. C++ 앱은 시작 시 우글거림 재생이 켜져 있으므로 `P`로 정지한 뒤 잰다(재생 중 대조군: 5초에 122회).

### 결과 (교대 실행 3회 × 100칸, 누락 0)

C++ 2.2.13은 `6c96784` 트리 Release(Qt 6.11.2), Rust는 presenter 구조(`ac95414` 이후) 빌드다.

| run | 앱 | 입력→present p50 / p95 (ms) | 입력→표시 p50 / p95 / max (ms) | 표시 방식 |
|---|---|---|---|---|
| 1 | C++ | 5.17 / 7.61 | 10.98 / 13.29 / 13.79 | Hardware Composed: Independent Flip |
| 1 | Rust | 2.25 / 4.29 | 11.72 / 15.23 / 16.07 (98/100 표시) | 대부분 Composed: Flip |
| 2 | C++ | 4.96 / 7.72 | 10.77 / 13.69 / 14.59 | Hardware Composed: Independent Flip |
| 2 | Rust | 2.35 / 4.14 | 12.16 / 14.87 / 16.86 | Composed: Flip |
| 3 | C++ | 4.40 / 8.16 | 10.12 / 13.96 / 15.51 | Hardware Composed: Independent Flip |
| 3 | Rust | 2.23 / 4.00 | 11.68 / 15.01 / 16.93 | Composed: Flip |

**현재 Rust 실증 앱은 C++보다 입력→표시가 p50 약 1ms, p95 약 1.3ms 느리다.** Rust는 present까지는 더 빠르지만, DWM이 swapchain을 한 번 더 합성한다(Composed: Flip). C++ 캔버스는 MPO 오버레이로 바로 표시된다. 이 차이를 없애는 것이 다음 과제다(4절).

Rust의 개별 칸 지연은 egui Painter를 쓰던 이전 빌드(`bd3981e`)와 같다(p50 11.60 / p95 14.87ms, 60칸). presenter가 개선한 것은 연속 입력이다. 약 2ms 간격 연속 이동에서 입력→present 반환이 p50 약 32ms에서 약 6ms로 줄었다. 입력→표시는 p50 11.64~11.79ms, p95 13.06~13.29ms(앱 자체 통계, 4회, 입력 502~506개 무손실)다. C++의 연속 입력 지연은 이 도구 방식으로는 아직 재지 않았다.

### 앱 자체 계측(DXGI 통계)과 교차 검증

`ugu_render::present::Presenter`는 present 직후 `GetLastPresentCount`로 번호를 기억한다. 이후 `GetFrameStatistics`가 그 번호를 화면에 띄운 present로 보고하면, 그 `SyncQPCTime`을 표시 시각으로 쓴다. 통계를 읽기 전에 교체된 present는 시각을 추정하지 않고 버린다.

같은 실행을 PresentMon과 프레임 단위로 짝지어 보았다(present 시각 2ms 이내, 4회). 결과는 다음과 같다.

- 연속 입력: 227/227, 228/228 짝지음
- 칸 패턴: 101/102, 102/103 짝지음
- DXGI 표시 시각 − PresentMon 표시 시각: 중앙값 −0.02~0.01ms, p95 2.0~2.5ms, 최대 3.0ms. 77~88%가 1ms 이내다.

즉 DXGI 통계는 같은 프레임에 대해 같거나 최대 3ms 늦게 보고한다(보수적).

### 측정 함정과 정정

- **PresentMon 입력 추적(기본값)은 측정 대상의 지연을 늘린다.** 같은 Rust 실행에서 입력→표시 p50이 11.7ms에서 18.0ms로 늘었다. 이 상태로 잰 이전 C++ 기록(p50 24.7~26.1 / p95 32.6~33.3ms)은 폐기한다. 도구는 이제 `--no_track_input`으로 실행한다.
- **"PresentMon이 wgpu DX12 present를 놓친다"는 이전 결론은 근거가 부족했다.** 현재 빌드는 대부분의 실행에서 모든 present가 기록된다. 다만 같은 조건에서도 가끔 실행 전체의 기록이 1~3프레임뿐인 경우가 재현되며, 원인은 아직 확정하지 못했다. 도구는 칸마다 응답 present 유무(`misses`)를 보고하므로, `misses`가 0이 아닌 실행은 버린다. 위 표의 실행은 모두 0이다.
- **로그 I/O가 렌더 스레드를 막았다.** 측정 스크립트가 앱 stdout을 끝까지 읽지 않은 채 trace·debug 로그를 켜면, 꽉 찬 파이프 때문에 렌더 스레드가 멈춰 present가 21~24회로 줄었다(입력 505개는 모두 도착). 앱은 이제 비차단 writer(`tracing-appender`)로 로그를 쓰므로, 로그 출력이 막혀도 그리기는 멈추지 않는다.
- **Desktop Duplication 방식은 폐기했다.** duplication이 켜지면 앱의 표시 방식이 Composed Flip으로 바뀌고, `LastPresentTime`은 표시가 아니라 present 시각에 가깝다. 이 방식의 기록(C++ p50 28ms, Rust 3.5ms)은 쓰지 않는다.

## 2. 렌더 기준선 (C++)

`ugurugu_render_benchmark`, `ugurugu_stress_document_generator 2048`(2,048², 4레이어, 2,000획, 200,000점, 11,875,879 bytes, 30프레임), DisplayPreview, 단일 스레드 전체 프레임:

| round | total (ms) | frame p50 (ms) | frame p95 (ms) | frame max (ms) |
|---|---|---|---|---|
| 0 | 140,790.8 | 4,604.62 | 4,633.98 | 4,635.75 |
| 1 | 140,776.6 | 4,603.49 | 4,633.46 | 4,633.83 |
| 2 | 140,730.4 | 4,605.45 | 4,628.72 | 4,646.93 |

digest는 세 round 모두 `30a14195fd945577`. 이 문서는 계획 10.2절 fixture ③(2,000획·200,000점)에 해당한다. 30fps 목표의 "2048² 표준 문서"(②)와 나머지 fixture 생성기는 아직 없다.

## 3. 입력 경로에서 확인한 사실

1. **winit은 펜 pointer 메시지를 `Touch`로 바꾸고 history 항목마다 현재 필압을 넣는다**(winit 0.30.13 `event_loop.rs` WM_POINTER 처리). barrel·eraser·hover도 없다. 그래서 `ugu-win`이 창을 subclass해 `WM_POINTER*`를 직접 받는다. `WM_POINTERCAPTURECHANGED`는 send 메시지라 message hook으로는 받을 수 없다.
2. **마우스 pointer 프레임에는 coalesced history가 없다**(`historyCount`가 항상 1). 이벤트 루프가 present를 기다리는 동안의 이동은 메시지 하나로 합쳐진다. `SendInput` 이동 약 500회 중 97~107개만 도착했다. `GetMouseMovePointsEx`로 복원한 뒤에는 3회 모두 505/506/505개(버튼 이벤트 포함)가 도착했고 궤적이 입력과 일치했다.
3. **절대 좌표 입력은 이동 버퍼와 커서 위치를 1px 다르게 반올림한다.** 정확한 (x, y, time) 질의는 대부분 실패한다. 같은 시각의 ±1px 이웃까지 찾고, 그 오프셋으로 복원점을 커서 좌표계에 맞춘다. 펜 태블릿의 마우스 모드 같은 실제 절대 장치도 같은 경로를 탄다.
4. **같은 밀리초에 여러 이동이 들어온다.** 복원점 시각은 ms 단위라 펜 history용 (시간, frame) 중복 제거에 넣으면 지워진다. 마우스는 버퍼 순서로만 정렬한다.
5. **`SetCursorPos`는 pointer 프레임을 만들지 않는다.** 자동 시험은 실제 입력 스트림인 `SendInput`을 써야 한다.
6. **egui-winit은 `RedrawRequested`에도 `repaint: true`를 돌려준다.** 이를 따라 다시 redraw를 요청하면 vsync마다 끝없이 그린다. 수정 전 Rust 앱은 유휴 상태에서도 16.66ms마다 present했고, 수정 후에는 유휴 5초 동안 0회다.

## 4. 프레임 순서 결정 (presenter로 해결)

연속 이동(약 2ms 간격) 중 앱 내부 지표(입력 시각 → present 반환)는 p50 약 32ms였다. Fifo 기본값, Fifo + frame latency 1, Mailbox 모두 같았고 Immediate만 0.6ms였다(tearing 때문에 제품 설정이 아님).

원인은 순서다. 현재 루프는 입력을 모은 뒤 swapchain 획득에서 vsync를 기다리므로, 대기 동안의 입력이 다음 프레임으로 밀린다. 캔버스 표시 경로는 **획득(대기) → 최신 입력 수집 → 렌더 → present** 순서를 가져야 하고, 이벤트 스레드는 present를 기다리지 않아야 한다. egui-wgpu `Painter`는 획득을 내부에서 하므로 쓰지 않는다.

구현 (`crates/ugu-render/src/present.rs`, `apps/ugurugu/src/render.rs`):

- wgpu DX12 옵션 `Dx12UseFrameLatencyWaitableObject::DontWait`로 wgpu 내부 대기를 끈다. 앱이 `Surface::as_hal`의 frame-latency 핸들을 직접 기다린 뒤에 입력을 읽는다. 최대 frame latency는 1이다.
- UI 스레드는 winit 루프와 포인터 subclass만 맡고, 창·포인터 이벤트를 채널로 렌더 스레드에 보낸다. 렌더 스레드가 GPU, egui, 캔버스를 소유한다.
- 순서: 메시지 대기 → 대기 핸들 신호 → 그동안 쌓인 입력 다시 수집 → egui 실행과 tessellate → 획득(막히지 않음) → 렌더 → present.
- winit은 UI 스레드 밖에서 창 핸들을 꺼내지 못하므로, wgpu instance와 surface는 UI 스레드에서 만들어 넘긴다.
- DXGI는 swapchain 작업 중에 창 스레드로 메시지를 보낼 수 있다. 그래서 UI 스레드는 창이 살아 있는 동안 렌더 스레드를 막고 기다리지 않고, 렌더 스레드의 종료 통지(`EventLoopProxy`)를 받은 뒤에 끝낸다.
- 표시 통계를 읽는 일만으로는 다시 그리지 않는다. 그러지 않으면 측정이 present를 늘린다.

### 남은 과제: Rust swapchain이 Independent Flip으로 승격되지 않음

Rust 창 전체 swapchain(wgpu DX12, BGRA8, client 크기와 같음)은 측정 중 계속 Composed: Flip이다. 아래 조건을 하나씩 바꿔 봤지만 모두 Composed였다.

- frame latency 1·2·3 (버퍼 2·3·4)
- 색 공간 `Auto`
- `WS_EX_NOREDIRECTIONBITMAP`
- OS 커서 숨김
- 버튼 없이 hover로 이동
- wgpu `DxgiFromVisual`(DirectComposition)

C++는 캔버스를 별도 자식 창(QRhi D3D11 swapchain)에 그린다. 다음 실험은 캔버스를 자식 창 swapchain으로 분리했을 때 승격되는지 확인하는 것이다. 이 구조는 egui 팝업이 캔버스 위로 그려지지 못하는 문제(airspace)를 함께 해결해야 하므로, 실험 결과를 보고 ADR에서 정한다.

## 5. 아직 측정하지 않은 기준선 항목

GUI 이벤트 batch, pen-up commit, 재생 fps(앱 내), 저장·열기, export 중 UI 반응, 취소, RAM. 앞의 세 항목과 취소는 2.2.13에 커밋된 계측 도구가 없으므로, 외부에서 재는 방법(WM_NULL ping, working set 샘플링, ETW)을 정한 뒤 측정한다. 유휴 상태 present 횟수는 1절에 기록했다.
