# M0 측정·실증 기록

환경은 [scope.md](scope.md) 1절(주 모니터 UHD 60Hz 150%, RTX 5080, 마우스만). 측정일 2026-10-06.

## 1. 입력→화면 표시 지연 기준선 (C++ 2.2.13)

도구: `tools/latency-probe` + PresentMon 2.6.0(Intel 서명 확인, 관리자 권한).

1. 도구가 PresentMon을 먼저 시작한 뒤 앱을 실행한다.
2. 창을 2560×1600 외곽 크기(client 2538×1549)로 맞춘다.
3. 주 버튼을 누른 채 캔버스 중앙의 나선(반경 = client 짧은 변 × 0.12) 획을 `SendInput`으로 한 칸씩 늘린다. 칸 사이에는 15~40ms 무작위 간격을 두어 vsync 위상과 상관을 없앤다.
4. 칸마다 다음 입력 전의 첫 present를 응답으로 본다. 지연은 `SendInput` 직전 QPC에서 그 present의 `TimeInQPC + MsUntilDisplayed`까지다.

이 값은 scanout 시작까지이며 패널 응답 시간은 포함하지 않는다. 앱 내부 계측이 필요 없다.

C++ 앱은 시작 시 우글거림 재생이 켜져 있으므로 `P`로 정지한 뒤 측정했다. 정지 상태에서는 유휴 5초 동안 present가 0회라서 "입력 뒤 첫 present = 응답"이 성립한다(재생 중 대조군: 5초에 122회).

3회 × 100칸 (C++ `6c96784` 트리 Release, Qt 6.11.2):

| run | 응답 | 입력→present p50 / p95 (ms) | 입력→표시 p50 / p95 / max (ms) |
|---|---|---|---|
| 1 | 99/100 | 9.44 / 16.43 | 25.74 / 32.60 / 33.22 |
| 2 | 100/100 | 9.75 / 16.96 | 26.06 / 33.25 / 34.18 |
| 3 | 100/100 | 8.54 / 16.92 | 24.68 / 33.04 / 33.64 |

표시 모드는 모두 Hardware Composed: Independent Flip이었다. 입력 후 present까지 평균 반 프레임, present 후 표시까지 약 한 프레임이다. 계획 10.2절의 "이벤트→present p95 33ms" 목표는 이 기준선(p95 약 17ms)보다 느슨하다. "현재 앱보다 나쁘지 않음" 조건이 실질적인 기준이다.

### 정정: Desktop Duplication 방식은 폐기

처음 만든 도구는 Desktop Duplication 화면에서 픽셀 변화를 찾았다. 이 방식에는 두 가지 문제가 있다.

- duplication이 켜져 있으면 앱의 표시 방식이 Independent Flip에서 Composed Flip으로 바뀐다. 즉 측정 대상이 달라진다.
- `LastPresentTime`은 표시 시각이 아니라 present 시각에 가깝다. 같은 시점 PresentMon으로 본 present→표시는 약 33ms였다.

이 방식으로 낸 이전 기록(C++ p50 28ms, Rust probe p50 3.5ms)은 쓰지 않는다.

### Rust 쪽은 앱 내부에서 측정

PresentMon은 wgpu DX12 앱의 present를 끝까지 추적하지 못했다. 아래는 같은 Rust 앱(입력마다 present 1회, 앱 상태 표시 102회)을 같은 조건에서 잡은 결과다.

| PresentMon 시작 시점 | 대상 지정 | 기록된 프레임 |
|---|---|---|
| 앱 실행 후 | PID | 2~5 |
| 앱 실행 후 | 프로세스 이름 | 2 |
| 앱 실행 전 | 프로세스 이름 | 32 (시작 후 약 0.6초에서 끊김) |

같은 절차에서 C++(D3D11)은 182~184프레임이 모두 기록됐다. 연속 입력(약 2ms 간격)에서는 Rust도 110프레임이 기록돼, 띄엄띄엄한 present에서만 생기는 문제로 보인다.

그래서 Rust 앱의 표시 지연은 `ugu_render::present::Presenter`가 DXGI 프레임 통계로 잰다. present 직후 `GetLastPresentCount`로 번호를 기억하고, 이후 `GetFrameStatistics`가 그 번호를 화면에 띄운 present로 보고하면 그 `SyncQPCTime`을 표시 시각으로 쓴다. 통계를 읽기 전에 다음 present로 교체된 present는 시각을 추정하지 않고 버린다.

### Rust M0 presenter 결과

C++와 같은 칸 패턴(나선 100칸, 15~40ms 무작위 간격, 2560×1600 창)을 SendInput으로 재현했다. 앱이 정상 종료할 때 남기는 자체 통계다(입력 샘플 102개, 표시 시각 확인 100~101프레임).

| run | 입력→present 반환 p50 / p95 (ms) | 입력→표시 p50 / p95 / max (ms) |
|---|---|---|
| 1 | 1.93 / 3.51 | 10.24 / 14.57 / 23.21 |
| 2 | 1.82 / 3.45 | 9.63 / 14.53 / 29.71 |
| 3 | 2.02 / 4.16 | 10.34 / 14.36 / 29.16 |
| 4 (리팩터링 뒤 재확인) | 1.82 / 3.85 | 10.15 / 14.23 / 23.02 |

연속 이동(약 2ms 간격 약 500회) 시험에서는 4회 모두 입력 502~506개가 도착해 무손실이었다. 입력→표시는 p50 11.64~11.79ms, p95 13.06~13.28ms였고, 입력→present 반환은 p50 6.04~6.17ms였다. 이전 구조에서는 present 반환까지만 해도 p50 약 32ms였다.

이 표시 시각은 C++ 기준선과 출처가 다르다(Rust는 DXGI 통계, C++는 PresentMon). 같은 실행을 두 방법으로 재는 교차 검증은 관리자 권한 세션에서 해야 하며, 아직 하지 않았다. 측정 시점의 세션이 관리자 권한이 아니었다. 이 Rust 화면은 egui 선 그리기뿐인 실증 캔버스이므로, 실제 렌더러를 넣은 뒤 다시 잰다.

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

## 5. 아직 측정하지 않은 기준선 항목

GUI 이벤트 batch, pen-up commit, 재생 fps(앱 내), 저장·열기, export 중 UI 반응, 취소, RAM. 앞의 세 항목과 취소는 2.2.13에 커밋된 계측 도구가 없으므로, 외부에서 재는 방법(WM_NULL ping, working set 샘플링, ETW)을 정한 뒤 측정한다. 유휴 상태 present 횟수는 1절에 기록했다.
