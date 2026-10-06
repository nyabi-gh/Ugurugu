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

C++는 캔버스를 별도 자식 창(QRhi D3D11 swapchain)에 그린다.

**가설: Windows 11의 둥근 창 모서리.** Rust swapchain은 client 영역 전체를 덮으므로 아래쪽 두 모서리가 둥글게 잘린다. DWM이 모서리를 잘라 내려면 swapchain을 직접 합성해야 하므로 오버레이 plane에 올릴 수 없다. C++ 캔버스 자식 창은 아래 상태 표시줄 위에서 끝나 모서리에 닿지 않는다. 위에서 바꿔 본 조건은 모두 모서리를 그대로 두었다. 최대화된 창은 모서리가 둥글지 않으므로, 이 가설이 맞으면 현재 구조도 최대화 상태에서는 승격된다.

실험 장치(`b1fffaa` 이후, 아직 측정하지 않음):

| `UGURUGU_PRESENT` | 구조 |
|---|---|
| (없음) | 창 전체 swapchain 하나 (현재) |
| `square` | 같은 구조에 `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND` |
| `child` | 캔버스만 자식 창 swapchain. 자식 창은 hit test를 부모로 넘기므로(`HTTRANSPARENT`) 포인터 입력 경로는 그대로다. 두 swapchain의 frame-latency 대기 핸들을 한 번에 기다리고, 캔버스를 먼저 present한다 |

`latency-probe`는 이제 swapchain별로 표시 방식과 지연을 따로 보고하고, `--window max`로 창을 최대화할 수 있다. 측정 순서(각 3회, C++와 교대, `misses` 0만 채택):

1. 기본 구조 × 복원 창 2560×1600: 기존 결과 재현 확인
2. 기본 구조 × `--window max`
3. `square` × 복원 창
4. `child` × 복원 창

`49b5809`부터 캔버스는 egui 도형이 아니라 Vello CPU 텍스처로 표시되므로, 1의 기존 결과 재현 확인도 이 빌드로 다시 잰다. 2·3이 Independent Flip이면 원인은 모서리 합성이다. 4만 승격되면 다른 조건(창 전체 크기, 비클라이언트 영역 인접 등)이 남는다.

### 구조 선택지 (측정 뒤 ADR로 확정)

| 구조 | 지연 | egui 팝업이 캔버스를 덮을 때 | 비용 |
|---|---|---|---|
| A. 창 전체 swapchain + 모서리 끔 | 모서리가 원인이면 C++와 같은 경로 | 문제 없음 (같은 swapchain) | 창 모서리가 각진다. 최대화 상태는 원래 각져 있다 |
| B. 캔버스 자식 창 (C++ 방식) | 승격 가능성이 가장 높음 | 자식 창이 팝업을 가린다(airspace). 팝업이 캔버스와 겹치는 동안 캔버스를 부모 swapchain으로 옮겨 그리거나, 팝업을 별도 최상위 창으로 띄워야 한다 | 전자는 그동안만 합성 경로로 돌아가므로 단순하다. 후자는 egui가 팝업 단위 viewport를 지원하지 않아 메뉴·콤보·툴팁을 직접 창으로 띄워야 한다 |
| C. DirectComposition 두 visual (캔버스 아래, UI 위) | UI visual이 알파로 위를 덮으므로 오버레이 승격은 underlay 지원 하드웨어에 한정 | 문제 없음 | 하드웨어 의존이 크고 검증 비용이 크다 |

측정 전 권고: 2·3에서 승격되면 A를 택한다. airspace 문제가 아예 생기지 않고, 최대화 사용에서는 시각 차이도 없다. B만 승격되면 B와 "팝업이 겹칠 때만 부모 swapchain에 그림" 대체 경로를 택한다. 팝업이 열린 동안에는 펜 입력이 거의 없으므로, 그동안 합성 경로로 돌아가 약 1ms 늦어지는 것은 허용 범위다.

## 5. 아직 측정하지 않은 기준선 항목

GUI 이벤트 batch, pen-up commit, 재생 fps(앱 내), 저장·열기, export 중 UI 반응, 취소, RAM. 앞의 세 항목과 취소는 2.2.13에 커밋된 계측 도구가 없으므로, 외부에서 재는 방법(WM_NULL ping, working set 샘플링, ETW)을 정한 뒤 측정한다. 유휴 상태 present 횟수는 1절에 기록했다.

## 6. CPU 렌더러 비교 (10~11일차)

도구: `tools/render-bench`(`6b3468c`). 같은 결정적 문서를 tiny-skia 0.12.0과 Vello CPU 0.3.0(`u8_pipeline`, 단일 스레드와 `multithreading`)으로 그린다. 외곽선은 도구가 한 번 만들어 두 렌더러에 같은 경로를 넘기므로, 차이는 래스터화와 합성에서만 생긴다.

- 문서: 2048², 5레이어(Normal·Multiply·Screen·Overlay, Overlay 레이어를 바닥으로 하는 클리핑 그룹 1개), 2,000획 × 100점 = 200,000점. 획의 50%는 펜(일정 폭, 둥근 끝·이음), 30%는 필압(점마다 원 + 이웃 사이 사각형, non-zero 합집합), 20%는 에어브러시(방사형 그라디언트 dab). 모두 반투명이다.
- 단계: 레이어 래스터화 30프레임(프레임마다 점을 ±2px 흔듦), 흰 배경 위 합성, 레이어 하나의 선택 변형(12° 회전·1.15배, bilinear) 10회, 실시간 획 한 칸(dirty rect 안에서만 그림) 500회.
- 프로세스 하나에 렌더러 하나만 돌려 peak working set·private bytes를 렌더러별로 잰다. 각 단계 결과 이미지를 raw RGBA와 PNG로 남기고 `render-bench diff`로 비교한다.

재현: `render-bench run --renderer tiny-skia|vello|vello-mt[=N] --out DIR`, `render-bench diff DIR/tiny-skia-composite.rgba DIR/vello-composite.rgba DIR/heat.png`.

### 정확성 (512², 100획, 기능 확인용 축소 실행)

| 비교 | 평균 채널 차 | 최대 | 차 > 2인 픽셀 | 차 > 8인 픽셀 |
|---|---|---|---|---|
| 합성 | 0.39 | 99 | 9.3% | 1.4% |
| 선택 변형 | 0.27 | 130 | 4.1% | 0.8% |
| 실시간 획 | 0.29 | 142 | 4.4% | 1.0% |

Vello 단일 스레드와 다중 스레드 결과는 바이트 단위로 같다. tiny-skia와의 차이는 획 가장자리의 안티에일리어싱(tiny-skia는 scanline 수직 4배 supersampling, Vello는 면적 coverage)과 에어브러시 그라디언트 내부에 몰려 있고, 모양이 빠지거나 밀린 곳은 없다(차이 heatmap 확인). 어느 쪽이 C++ QPainter에 가까운지는 아직 비교하지 않았다. 2.2.13과의 비교는 같은 문서를 C++로 그리는 생성기가 있어야 한다.

### 2048² 전체 실행 (교대 3회)

조건: 24 논리 코어, 프로세스 우선순위 BelowNormal(사용자가 게임 중이었다). `vello-mt`는 기본 스레드 수 8(min(코어 − 1, 8))이다. 세 라운드의 p50 범위가 좁고, 같은 렌더러의 출력은 라운드 사이에 바이트 단위로 같다.

| 단계 (ms) | tiny-skia p50 / p95 | Vello 1스레드 p50 / p95 | Vello 8스레드 p50 / p95 |
|---|---|---|---|
| 래스터화 (5레이어 전체, 프레임당) | 1,138.8~1,159.2 / 1,176.6~1,183.0 | 336.2~348.8 / 357.9~374.9 | 45.4~48.7 / 57.1~62.0 |
| 합성 | 85.6~85.7 / 87.5~89.6 | 55.0~56.1 / 63.7~65.3 | 8.0~8.2 / 8.4~8.8 |
| 선택 변형 | 39.3~41.2 / 40.7~42.5 | 24.7~25.5 / 25.6~26.1 | 2.7~2.9 / 3.0~3.6 |
| 실시간 획 한 칸 | 0.006~0.007 / 0.009~0.010 | 0.003 / 0.004~0.005 | 0.114~0.131 / 0.236~0.261 |
| peak working set (MiB) | 208.7 | 211.0~211.4 | 222.4~240.5 |
| peak private (MiB) | 213.9~214.0 | 221.5~222.3 | 235.5~254.2 |

외곽선 생성(두 렌더러 공통, 프레임당)은 6.5~8.2ms다. 메모리의 대부분은 2048² 이미지 7장(약 112MiB)이다.

정확성(2048², 1라운드): tiny-skia 대 Vello의 합성은 평균 채널 차 0.85, 최대 139, 차 > 2 픽셀 20.5%, 차 > 8 픽셀 2.9%다. 선택 변형은 0.59 / 127 / 10.1% / 1.7%, 실시간 획은 0.58 / 134 / 10.8% / 1.8%다. 512²보다 비율이 큰 것은 획이 더 촘촘하기 때문이다. heatmap에서 차이는 획 가장자리와 반투명 영역의 반올림에만 있고, 블렌드 결과가 면 단위로 다른 곳은 없다.

해석:

- Vello CPU는 1스레드에서도 래스터화 3.3~3.4배, 합성 1.5배, 변형 1.6배 빠르고, 8스레드에서는 tiny-skia 대비 래스터화 약 24배, 합성 약 10.5배다. 메모리는 비슷하다(다중 스레드 +10~25MiB).
- 실시간 획 한 칸은 두 렌더러 모두 수 µs로 입력 지연에 영향이 없다. 단 Vello 다중 스레드 context는 작은 작업에 분배 비용(약 0.1ms)이 더 크므로, 실시간 획은 1스레드 context로, 재생·전체 다시 그리기는 다중 스레드 context로 나눈다.
- 다른 문서라 직접 비교할 수는 없지만 참고로 C++ stress fixture ③(4레이어, 2,000획)은 프레임당 4.6s였다. 같은 규모에서 Vello 8스레드는 래스터화와 합성을 합쳐 약 55ms다.
- 남은 위험: `vello_cpu`는 0.x라 API가 바뀔 수 있다(이 비교는 0.3.0으로 고정). 어느 쪽이 C++ QPainter 결과에 가까운지는 아직 비교하지 않았다.

### SIMD 수준과 결정성

Vello CPU는 실행 시 CPU의 SIMD 수준을 골라 쓰므로, CPU가 다르면 픽셀이 달라질 수 있는지 확인했다(`render-bench --simd`, 이 PC가 가진 SSE2·SSE4.2·AVX2).

- SSE2와 SSE4.2는 모든 단계에서 바이트 단위로 같다(768², 400획).
- AVX2는 다르다. 2048²에서 합성 20픽셀(최대 ±2), 실시간 획 15픽셀, 선택 변형 25픽셀(±1)이며 알파는 같다. 원인은 방사형 그라디언트(`fine/common/gradient/radial.rs`)의 `mul_add`다. AVX2 경로는 이를 FMA 한 번(반올림 1회)으로, SSE 경로는 곱하기 뒤 더하기(반올림 2회)로 계산한다.
- SSE 경로는 Intel과 AMD에서 결과가 다른 근사 명령(`rcpps`·`rsqrtps`)을 쓰지 않는다. `sqrt`는 IEEE 규격상 정확히 반올림된다. 따라서 수준을 고정하면 CPU 제조사와 관계없이 같은 결과가 나온다.

속도(2048², 교대 3회, p50 ms):

| 단계 | SSE4.2 1스레드 | AVX2 1스레드 | SSE4.2 8스레드 | AVX2 8스레드 |
|---|---|---|---|---|
| 래스터화 | 351.0~357.8 | 341.1~346.7 | 48.3~51.9 | 48.0~49.6 |
| 합성 | 41.8~43.0 | 55.4~56.9 | 5.2~5.5 | 8.0~8.1 |
| 선택 변형 | 21.6~22.4 | 23.3~25.1 | 2.9~3.0 | 2.7~3.0 |

SSE4.2는 1스레드 래스터화가 약 3% 느리고 합성은 오히려 25~35% 빠르다. 그래서 문서 렌더러는 SSE4.2로 고정한다(`ugu_render::raster::document_level`, Windows 11 24H2부터 필수 명령어 집합). SSE4.2가 없으면 같은 결과를 내는 SSE2로 떨어진다. 다른 PC 사이의 일치는 이 PC 안에서 수준을 바꿔 확인한 것이고, 다른 제조사 CPU에서 직접 비교한 것은 아니다.

### Vello GPU (준비, 시간 측정은 남음)

`render-bench --renderer vello-gpu`(Vello GPU 0.3.0, wgpu 30 DX12, RTX 5080)가 같은 문서와 단계를 GPU 텍스처 위에서 수행한다. 레이어는 GPU 텍스처에 남고, 각 단계는 GPU 완료까지 기다린 시간을 잰다. 256², 50획 기능 확인에서 Vello CPU와의 차이는 합성 평균 0.26·최대 9, 선택 변형 최대 7, 실시간 획 최대 7이다. 같은 알고리즘이라 tiny-skia보다 훨씬 가깝지만 바이트 단위로 같지는 않다. 따라서 재생에 GPU를 쓰고 내보내기에 CPU를 쓰면 이 정도 차이가 남는다. 2048² 시간 측정은 GPU를 다른 프로그램이 쓰지 않을 때 한다.
