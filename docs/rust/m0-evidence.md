# M0 측정·실증 기록

환경은 [scope.md](scope.md) 1절(주 모니터 UHD 60Hz 150%, RTX 5080, 마우스만). 측정일 2026-10-06.

## 1. 입력→화면 지연 기준선

도구: `tools/latency-probe` (Desktop Duplication). 대상 창을 2560×1600 외곽 크기(client 2538×1549)로 맞추고, 주 버튼을 누른 채 캔버스 중앙의 나선(반경 = client 짧은 변 × 0.12) 획을 한 칸씩 늘린다. 각 칸은 `SendInput` 직전 QPC에서 그 부분 픽셀이 처음 바뀐 desktop 프레임의 `LastPresentTime`까지다. 칸 사이에 15~40ms 무작위 간격을 두어 vsync 위상과 상관을 없앴다. 하드웨어 커서는 duplication 화면에 포함되지 않는다.

이 값은 **DWM이 새 desktop 이미지를 받은 시각까지**이며 photon까지가 아니다. 앱 내부 계측이 필요 없어 두 앱을 같은 방식으로 잰다. 이전 문서의 "input→present-return 3.7ms"(project-status)는 앱 안에서 present 호출 반환까지 잰 다른 지표라 직접 비교하지 않는다.

C++ 2.2.13(`6c96784` 트리 Release, Qt 6.11.2)은 시작 시 우글거림 재생이 켜져 있어 `P`로 정지한 뒤 측정했다. 재생 중에는 획 외 픽셀도 매 프레임 바뀌어 이 방식이 성립하지 않는다.

교대 실행 3회 × 100칸, 누락 0:

| 앱 | p50 (ms) | p95 (ms) | max (ms) | min (ms) |
|---|---|---|---|---|
| C++ 2.2.13 | 28.42 / 28.60 / 28.71 | 33.66 / 33.68 / 33.37 | 34.53 / 34.36 / 34.29 | 17.88 / 17.68 / 17.83 |
| Rust M0 probe (`9cac795`) | 4.16 / 3.03 / 3.29 | 6.87 / 6.24 / 6.94 | 7.34 / 7.47 / 7.78 | 0.57 / 0.51 / 0.79 |

Rust 쪽은 egui 선 그리기뿐인 입력 실증 캔버스이며 렌더러가 없다. 같은 지표로 비교할 수 있다는 것과 이 경로의 구조적 하한만 보여 준다. 렌더러를 넣은 뒤 다시 잰다.

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

## 4. 렌더 구조에 넘길 결정 사항

연속 이동(약 2ms 간격) 중 앱 내부 지표(입력 시각 → present 반환)는 p50 약 32ms였다. Fifo 기본값, Fifo + frame latency 1, Mailbox 모두 같았고 Immediate만 0.6ms였다(tearing 때문에 제품 설정이 아님).

원인은 순서다. 현재 루프는 입력을 모은 뒤 swapchain 획득에서 vsync를 기다리므로, 대기 동안의 입력이 다음 프레임으로 밀린다. 캔버스 표시 경로는 **획득(대기) → 최신 입력 수집 → 렌더 → present** 순서를 가져야 하고, 이벤트 스레드는 present를 기다리지 않아야 한다. egui-wgpu `Painter`는 획득을 내부에서 하므로 캔버스 표시에는 직접 관리하는 surface가 필요하다. 렌더러 비교(10~11일차)와 ADR에서 확정한다.

## 5. 아직 측정하지 않은 기준선 항목

GUI 이벤트 batch, pen-up commit, 재생 fps(앱 내), 저장·열기, export 중 UI 반응, 취소, RAM, 유휴/최소화. 앞의 세 항목과 취소·유휴는 2.2.13에 커밋된 계측 도구가 없다. 외부에서 재는 방법(WM_NULL ping, working set 샘플링, ETW)을 정한 뒤 측정한다. PresentMon(FrameView 동봉)은 관리자 권한이나 Performance Log Users 그룹이 필요하다.
