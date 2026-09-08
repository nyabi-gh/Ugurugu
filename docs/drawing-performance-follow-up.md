# Ugurugu 그리기 성능 후속 작업

- 기준일: 2026-08-18
- 기준 커밋: `a5a8c36d66ff8a57d30ded57183439416bd34fd9`
- 기준 릴리스: 2.2.10 게시됨(`debf530`, 2026-08-18)
- 범위: Qt 데스크톱 앱의 그리기 입력, 미리보기, 프레임 캐시와 배포 검증
- 제외 범위: 웹 이식, `.ugu` 파일 형식 변경, 근거 없는 GPU renderer 재작성

> 상태 정리(2026-09-08): **진행 중**. 2.2.10 게시와 P1 두 건의 구현·회귀
> 테스트 추가는 완료됐다. P1 정량 A/B, 저장소 내 성능 probe, P2 계측과 실제
> 업데이트 smoke는 완료 기록이 없다. P3 GPU 작업은 조건부 보류다.

## 1. 현재 상태

2.2.9 이후 제품 변경은 `05276c8`과, 2026-08-18에 구현한 아래 P1 두 건
`9ce634c`(성공 승격 뒤 중복 interaction warmup 제거)와 `fe51f80`(취소를
RenderEngine 내부까지 전달)이다. P1 두 건은 사용자 결정으로 2.2.10에
포함한다. 회귀 테스트는 갖췄지만 아래 완료 조건의 Release 50회 A/B 측정은
아직 수행하지 않았다. `05276c8`은 움직임 재생을 멈춘 동안에는
현재 프레임만 준비하고, 나머지 프레임의 캐시 갱신은 재생을 다시 시작할 때
수행한다. 구현 위치는
[`CanvasWidget::setAnimating`](../src/ui/CanvasWidget.cpp)과
[`CanvasWidget::scheduleFrameCacheWarmup`](../src/ui/CanvasWidgetPreview.cpp)이다.

`Happy_birthday.ugu`와 바이트가 같은 복구본을 사용한 macOS Cocoa Release
측정은 software 표시, 회전 0도, 우글거림 켬, 재생 정지 조건에서 변경 전후를
각 24회 비교했다.

| 지표 | 변경 전 | 2.2.10 | 판정 |
|---|---:|---:|---|
| 다음 획 직전 frame-cache worker | 8 | 0 | 목표 달성 |
| 획당 process CPU p50 | 258.64 ms | 126.05 ms | 51% 감소 |
| 획별 최장 live-display event p50 | 9.83 ms | 5.38 ms | 45% 감소 |
| 획별 최장 live-display event p95 | 12.10 ms | 7.40 ms | 39% 감소 |
| 획 전체 wall p50 | 81.04 ms | 70.53 ms | 13% 감소 |
| 획 전체 wall p95 | 92.81 ms | 91.92 ms | 유의미한 차이 없음 |
| pen-up wall p50 | 0.79 ms | 0.84 ms | 개선 근거 없음 |

회귀 테스트는 정지 중 현재 프레임을 전체 렌더 결과와 픽셀 단위로 비교하고,
다른 29프레임에는 worker가 생기지 않으며, 재생 재개 후 UI-thread 동기 렌더
없이 모두 갱신되는 것을 고정한다
([`UiViewportTests.cpp`](../tests/UiViewportTests.cpp)).

이 결과로 다음 판단은 닫는다.

- 매 획의 전체 JSON 직렬화가 주 병목은 아니다. 일반 stroke commit은 증분
  직렬화를 사용하고, 자동 복구의 전체 직렬화와 파일 쓰기는 전용 worker에서
  실행된다.
- `.ugu` 스키마나 파일 형식을 바꿀 이유가 없다.
- CPU 렌더러 전체를 GPU로 옮기는 것은 다음 작업이 아니다. 현재 확인된 가장
  큰 잔여 비용부터 줄인 뒤 프로파일 결과로 다시 판단한다.

## 2. 기술 후속 작업

| 우선순위 | 상태 | 작업 | 다음 결정 기준 |
|---|---|---|---|
| P1 | 구현 완료(`9ce634c`), A/B 측정 대기 | 성공한 stroke 뒤 중복 interaction warmup 제거 | worker·픽셀·fallback A/B |
| P1 | 구현 완료(`fe51f80`), A/B 측정 대기 | cancellation을 RenderEngine 내부까지 전달 | 취소 뒤 잔여 CPU와 정상 렌더 회귀 |
| P2 | 병목 여부 미측정 | regional patch의 UI-thread 전체 프레임 복사 | callback p95와 복사 바이트 |
| P2 | 경합 여부 추가 측정 | 백그라운드 우선순위와 동시성 | 입력 latency와 resume 준비 시간 |
| P3 | 보류 | GPU 작업 | Metal profile에서 반복 지배 비용 확인 |

### P1. 성공한 stroke 뒤 중복 interaction warmup 제거

정지 상태의 frame-cache worker는 없어졌지만, 현재 프레임을 준비하는 단일
interaction worker는 남아 있다. idle 조건 24회에서 이 작업은 wall
p50/p95/max `8.00/8.22/8.24 ms`, process CPU `6.59/7.47/7.81 ms`였다.
immediate 조건의 다음 획과 겹칠 가능성은 있지만, 현재 측정만으로 체감 지연의
원인이라고 확정하지 않는다.

권장 구현 경계:

1. pen-up 결과가 정확한 current frame과 재사용 가능한 layer split 또는
   layer-raster base를 모두 승격한 경우에만 commit 도중 예약된 interaction
   작업을 취소하거나 처음부터 예약하지 않는다.
2. resampled commit, 승격 실패, frame만 있고 재사용 base가 없는 fallback,
   일반 cache invalidation, 정지 중 frame 이동에는 interaction 준비를 유지한다.
3. 그리는 동안 계속 우글거리기 기능의 다음 프레임 준비도 유지한다.

완료 조건:

- Release에서 문제 문서와 빈 문서, immediate와 idle을 각각 50회 이상 측정한다.
- 성공 승격 경로에서 다음 획 직전 interaction worker가 0이고 동기 full preview
  render 횟수가 늘지 않는다.
- 승격한 현재 프레임과 `RenderEngine::renderScaled` 결과가 픽셀 단위로 같다.
- resampled, split/raster 부재, 일반 invalidation, frame scrub에 대한 회귀
  테스트가 각각 interaction fallback을 확인한다.

### P1. 취소를 RenderEngine 내부까지 전달

현재 frame-cache 취소는 generation과 atomic flag를 바꾸지만 렌더 호출 전후에만
확인한다. 이미 시작한 작업은 한 frame의 raster와 합성을 끝낼 수 있다. 2.2.10은
정지 중 새 작업을 만들지 않아 발생 빈도를 크게 줄였지만, 재생 중 warmup을
정지하거나 warmup 직후 펜을 내리는 경우에는 이전 worker가 잠시 남을 수 있다.

권장 방향:

- `renderScaled`, `renderScaledRegion`, layer replay와 hierarchy compositor에
  읽기 전용 cancellation token을 전달한다.
- layer, operation 또는 tile 경계에서 취소를 확인한다. primitive마다 검사해
  정상 렌더 hot path를 느리게 만들지 않는다.
- 취소된 결과는 부분 이미지로 취급하지 않고 명시적인 cancelled 상태로
  반환하며 cache에 채택하지 않는다.

완료 조건:

- pause 또는 pen-down 뒤 취소된 job의 CPU가 한 frame 전체 렌더 시간만큼 계속
  증가하지 않는다.
- 취소하지 않은 렌더의 픽셀 digest와 성능이 기존 결과와 같다.
- 빠른 pause/resume과 연속 pen-down에서 이전 generation 결과가 채택되지 않는다.

### P2. regional patch의 UI-thread 전체 프레임 복사 측정 및 이동

regional worker가 patch를 만든 뒤 UI callback에서 cached base를 `QImage::copy()`로
전체 복사하고 patch를 적용한다. 큰 캔버스에서는 worker 렌더보다 이 메모리
복사가 입력 이벤트를 늦출 수 있지만, 현재 프로파일로 지배 비용임을 확인하지
못했다.

먼저 callback의 wall/UI CPU, 복사 바이트, 대기 중인 입력 이벤트를 계측한다.
병목이 확인되면 worker에서 완성 이미지를 만들고 UI thread는 generation 확인과
cache pointer 교체만 하도록 바꾼다. QImage implicit sharing, 취소된 generation,
동시 peak memory와 preview cache budget을 함께 검증해야 한다.

완료 조건은 UI callback p95와 최대값, 전체 복사 바이트가 전후 표에 포함되고,
4K 문서에서 메모리 예산을 넘지 않으며 픽셀 결과가 같은 것이다.

### P2. 백그라운드 작업 우선순위와 동시성

frame cache는 최대 8 workers, interaction 준비는 1 worker, 자동 복구는 별도
thread를 사용한다. 정지 중 frame-cache 경쟁은 제거됐으므로 즉시 worker 수를
줄이지 말고 다음을 계측한 뒤 결정한다.

- 재생 중 cache warmup과 실제 입력이 겹칠 때 UI latency와 process CPU
- interaction 작업이 frame-cache 슬롯 또는 메모리 대역폭을 기다리는 시간
- RecoveryWriter 직렬화가 동시에 실행될 때의 변화

필요하면 active input과 current interaction을 최우선으로 두고, frame cache는
가변 concurrency 또는 낮은 우선순위로 실행한다. 재생 재개 후 준비 시간이
과도하게 늘지 않는 것을 guardrail로 둔다.

### P3. GPU는 전체 이관이 아니라 계측 후 좁게 검토

현재 GPU 표시는 CPU가 만든 QImage를 texture로 올리고 quad를 그린다. stabilizer,
history, 직렬화, layer 계획은 GPU로 옮겨도 이득이 없고, CPU raster와 hierarchy
합성을 전부 다시 작성하면 픽셀 결정성, software fallback과 유지보수 비용이
크게 늘어난다.

Metal System Trace에서 upload 또는 합성이 반복해서 지배 비용으로 확인될 때만
dirty-bound texture 합성 같은 좁은 후보를 검토한다. GPU 경로에는 software와의
픽셀 동등성, GPU 실패 fallback, 선택/clip/blend 회귀 테스트가 선행되어야 한다.

## 3. 측정 인프라와 미확인 항목

현재 상세 probe는 저장소 밖 임시 도구였다. 다음 성능 변경 전에 재현 가능한
native desktop probe를 `tools/`에 추가하되 실제 사용자 `.ugu`는 저장소에
커밋하지 않는다. 외부 문서 경로와 SHA-256을 입력받고 결과를 JSON으로 남기는
방식이 적절하다.

probe가 최소한 기록할 항목:

- live input, 강제 display, pen-up, post-display의 wall/UI CPU/process CPU
- frame-cache와 interaction worker 수, cache/missing/stale frame 수
- split/raster/fallback 선택, 동기 full render 횟수와 dirty/upload bytes
- immediate/idle, rotation 0/5도, software/GPU, wobble on/off, 기존/빈 문서
- 각 cell 50회 이상, 실행 순서 무작위화, 열 상태와 timeout 기록

아직 하지 못한 검증:

- 실제 태블릿의 OS event 도착부터 화면 presentation까지 end-to-end 지연
- 2.2.10 변경 후 Instruments Time Profiler와 Metal System Trace
- post-change 전체 조건 행렬. 2.2.10 A/B는 software, 0도, wobble on의
  immediate/idle만 다시 측정했다.
- Windows 실제 표시 경로와 updater end-to-end

모든 CTest는 `QT_QPA_PLATFORM=offscreen`이고 package smoke는 canvas를 실제로
표시하지 않는다. 태블릿 테스트도 합성 `QTabletEvent`를 사용한다. 따라서 위
항목은 실패가 아니라 해당 GPU·장치·드라이버 환경에서 아직 실행하지 않은
범위다.

## 4. 2.2.10 릴리즈 운영

로컬에서는 다음을 완료했다.

- [x] `2.2.10` 버전과 ko/en/ja 릴리즈 노트 작성
- [x] macOS Release와 Distribution 전체 테스트 13/13
- [x] Distribution warnings-as-errors 빌드
- [x] 설치 앱 package smoke, JPEG plugin, 라이선스, ad-hoc codesign 검증
- [x] 세 언어 release-note HTML 변환과 앱 bundle version 확인

외부 릴리즈 운영은 아래 체크 상태까지 완료됐다. 실제 업데이트 검증은 남아 있다.

- [x] release 준비 커밋과 이 문서를 원격에 반영한다.
- [x] 같은 SHA의 main CI가 macOS와 Windows에서 모두 통과하는지 확인한다.
      (`debf530`, 12 jobs green)
- [x] 기존 관례와 같은 lightweight tag `v2.2.10`을 만들고 push한다.
      (`debf530`에 지정)
- [x] tag의 release workflow를 실행한다. (run 32116702894)
- [x] macOS notarization/stapling과 Windows Velopack job이 성공했는지
      확인한다. (4 jobs success, dmg·zip·appcast·nupkg·RELEASES 게시)
- [ ] GitHub Release의 ko/en/ja 본문과 Sparkle/Velopack 업데이트 창을
      확인한다. 본문 3개 언어는 확인했고, 실제 업데이트 창은 아래 smoke와
      함께 남아 있다.
- [ ] 2.2.9 설치본에서 2.2.10으로 실제 업데이트하고 서명·실행·문서 열기·
  그리기를 smoke test한다.

2.2.10에는 `05276c8`과 P1 두 건(`9ce634c`, `fe51f80`)을 포함한다. P1 두
건은 회귀 테스트까지 마쳤고, 정량 A/B는 릴리즈 후 native desktop probe를
추가하면서 수행한다. P2 이후 항목은 별도 버전에서 하나씩 A/B한 뒤 반영한다.

## 5. 권장 실행 순서

1. 게시된 2.2.10의 실제 업데이트·태블릿 smoke를 기록한다.
2. native desktop probe를 저장소에 추가해 같은 조건을 재현 가능하게 만든다.
3. 성공 승격 뒤 중복 interaction warmup만 격리해 A/B한다.
4. 구현된 render loop 취소 token의 잔여 CPU와 정상 렌더 비용을 A/B한다.
5. 프로파일이 입증할 때만 UI-thread patch copy와 scheduler를 바꾼다.
6. CPU 변경 후에도 남은 병목이 GPU upload/합성으로 확인될 때만 GPU 작업을
   검토한다.

## 6. 갱신 규칙

- 항목을 완료할 때 관련 commit, Release 빌드와 테스트 결과, 측정 조건과
  p50/p95/max를 함께 기록한다.
- 확인된 사실, 아직 측정하지 않은 항목과 가설을 구분한다. 새 프로파일이 기존
  가설을 반박하면 오래된 권고를 유지하지 않는다.
- 실제 사용자 문서는 fixture로 커밋하지 않는다. 재현에는 외부 경로와
  SHA-256을 사용하고, 필요하면 별도의 비식별 synthetic fixture를 만든다.
- 릴리즈가 게시되면 운영 checklist를 결과와 artifact 링크로 갱신하고, 다음
  성능 변경은 한 번에 한 원인만 격리해 측정한다.
