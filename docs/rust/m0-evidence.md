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

Rust 앱의 표시 지연은 직접 만들 presenter에서 DXGI 프레임 통계(`GetFrameStatistics`의 present 번호와 `SyncQPCTime`)로 잰다. presenter는 4절 때문에 어차피 직접 만들어야 한다.

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

## 4. 렌더 구조에 넘길 결정 사항

연속 이동(약 2ms 간격) 중 앱 내부 지표(입력 시각 → present 반환)는 p50 약 32ms였다. Fifo 기본값, Fifo + frame latency 1, Mailbox 모두 같았고 Immediate만 0.6ms였다(tearing 때문에 제품 설정이 아님).

원인은 순서다. 현재 루프는 입력을 모은 뒤 swapchain 획득에서 vsync를 기다리므로, 대기 동안의 입력이 다음 프레임으로 밀린다. 캔버스 표시 경로는 **획득(대기) → 최신 입력 수집 → 렌더 → present** 순서를 가져야 하고, 이벤트 스레드는 present를 기다리지 않아야 한다. egui-wgpu `Painter`는 획득을 내부에서 하므로 캔버스 표시에는 직접 관리하는 surface가 필요하다. 렌더러 비교(10~11일차)와 ADR에서 확정한다.

## 5. 아직 측정하지 않은 기준선 항목

GUI 이벤트 batch, pen-up commit, 재생 fps(앱 내), 저장·열기, export 중 UI 반응, 취소, RAM. 앞의 세 항목과 취소는 2.2.13에 커밋된 계측 도구가 없으므로, 외부에서 재는 방법(WM_NULL ping, working set 샘플링, ETW)을 정한 뒤 측정한다. 유휴 상태 present 횟수는 1절에 기록했다.
