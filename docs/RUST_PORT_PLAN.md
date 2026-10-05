# Ugurugu Rust 전면 포팅 계획

- 기준: `main` `61ab90a` (2.2.13 준비), 2026-10-05
- 범위: Windows·macOS·Linux 데스크톱과 웹. 모바일은 제외한다. Qt는 데스크톱과 웹 모두에서 완전히 제거한다. 데스크톱 UI는 egui + wgpu를 우선 검토한다.
- 확정된 전제 (2026-10-05 사용자 결정)
  - Rust 판은 C++ 판의 `.ugu`를 지원하지 않는다. 레거시를 털어내는 것이 목적이므로, 더 나은 설계가 있으면 그쪽으로 간다.
  - 단, **그림이 그려지는 느낌과 우글거리는 느낌은 크게 변하면 안 된다.**
  - 웹도 Qt를 걷어낸다(Qt for WebAssembly 제거).
  - macOS 업데이터는 Sparkle을 유지한다.
  - Rust 데스크톱은 새 앱 정체성(앱 id·설치 위치·확장자·업데이트 피드)으로 낸다.
  - WiggleWiggleTool `.wawa` 가져오기는 넣지 않는다.
- 방법: 소스 정적 분석(모듈별 Qt 식별자 집계, 렌더·포맷·UI·빌드·CI 경로 확인)과 기존 문서([ANALYSIS_REPORT.md](ANALYSIS_REPORT.md), [project-status.md](project-status.md)) 대조. 이 계획 단계에서는 빌드나 측정을 하지 않았다. 성능과 느낌에 관한 판단은 측정 전 가설이며, 0단계에서 측정으로 확정한다.
- 진행 상태는 이 문서가 아니라 `docs/project-status.md`에서만 갱신한다. 이 문서는 설계 근거와 단계 정의다.

---

## 요약 — 질문별 결론

| 질문 | 결론 |
|---|---|
| core / renderer / IO / UI 분리 | 크레이트 의존 방향으로 강제한다. **core**는 모델·편집·히스토리(순수, 파일·스레드·시계 없음), **renderer**는 결정적 CPU 렌더(헤드리스, 모든 OS와 wasm에서 같은 픽셀), **IO**는 새 파일 형식과 내보내기(바이트 ↔ 모델, 파일 시스템 없음), **UI**는 UI 비의존 세션 계층 + wgpu 표시 + 얇은 egui 셸로 나눈다. 웹은 같은 세션 계층을 wasm으로 쓴다. (§3.1–3.2) |
| `QImage`·`QPainter`·`QTransform`·컨테이너 대체 | 레거시 호환 부담이 없으므로 표준적인 선택을 한다. 기하는 `kurbo`(f64 점·사각형·아핀·베지어), 이미지는 자체 premultiplied RGBA8 버퍼(`Arc` COW, 명시적 이미지 id), 그리기는 이 앱의 브러시에 맞춘 자체 해석적 래스터라이저, 컨테이너는 `Vec`/`Arc<[T]>`/`HashMap`/`BTreeMap`, 신호는 커밋이 반환하는 이벤트 목록. (§3.3) |
| wgpu로 바로? CPU를 거쳐서? | **CPU 렌더러를 정답으로 둔다.** 저장·내보내기 픽셀이 기기와 OS에 관계없이 같아야 하고, CI와 웹(wasm)에는 GPU가 보장되지 않기 때문이다. wgpu는 표시(프레임 텍스처, 줌·회전, 체커보드, 오버레이)를 맡는다. 새 래스터라이저는 타일 단위 해석적 계산이라, 측정으로 필요가 확인되면 같은 수식을 GPU 미리보기로 옮길 수 있다. (§3.5) |
| egui와 캔버스 연결 | 직접 작성한 winit 루프 + egui-winit + egui-wgpu. 캔버스는 `egui_wgpu` paint callback으로 egui 렌더 패스 안에서 그리고, 더티 영역 업로드는 `prepare` 단계에서 한다. 펜 샘플은 egui를 거치지 않는 별도 스트림으로 세션 계층에 들어간다. (§3.6) |
| C++ ↔ Rust 결과 검증 | 비트 동일이 아니라 **느낌 동일**을 검증한다. 우글거림 기하(변위 계산)는 같은 알고리즘을 옮겨 수치로 일치시키고, 래스터는 커버리지·가장자리·불투명도 누적을 지표로 비교하며, 마지막 판정은 사람이 나란히 보고 한다. 허용치는 지금 C++ 앱이 Windows와 macOS에서 이미 서로 다르게 그리는 정도(OS libm 차이)를 기준으로 잡는다. C++는 이 비교가 끝날 때까지만 참조 도구로 쓴다. (§4) |
| 위험이 가장 낮은 시작점 | 우글거림 수학(노이즈·모션 3종·끊어진 선, 약 0.8K줄)과 1€ 필터. 같은 알고리즘을 옮기므로 C++와 수치 비교가 쉽고, 우글거리는 느낌을 처음부터 고정한다. 가장 위험한 것은 래스터라이저의 그리는 느낌과 펜 입력·지연이며, 0단계 스파이크에서 먼저 측정한다. (§5) |
| main을 계속 쓸 수 있게 | 같은 저장소에 Cargo 워크스페이스를 추가하고, C++ 데스크톱 앱은 Rust 데스크톱이 나올 때까지 지금처럼 출시한다. 웹은 Rust wasm 엔진이 브라우저 시나리오를 통과하면 엔진을 교체한다. 파일 형식이 달라지므로 Rust 데스크톱은 **별도 앱 정체성**(앱 id·확장자·업데이트 피드)으로 낸다. (§6) |

**가장 중요한 판단: "느낌"을 이루는 것과 이루지 않는 것을 나눈다.**

| 지켜야 하는 것 (느낌) | 바꿔도 되는 것 (구현) |
|---|---|
| 우글거림 변위 수식과 상수(Classic·Smooth·Stepped, 디테일·연결·무작위성, 끊어진 선), seed별 결정성 | 래스터 알고리즘(Qt 래스터 엔진 재현 불필요) |
| 획 기하: 리샘플 간격, 필압 → 굵기·불투명도 곡선, round/square 팁 모양, 비AA 기본값의 또렷한 가장자리 | 파일 형식, 직렬화, 예산 계산 방식 |
| 에어브러시의 경도·흐름에 따른 누적 방식, 스프레이 입자 분포 | 픽셀 메모리 배치, 좌표·변환 타입 |
| 8비트 합성에서 나오는 누적 특성(저불투명 dab이 쌓이는 모양) | 타일·캐시·스레딩 구조 |
| 손떨림 보정(1€ 필터) 응답, 최소 점 간격, 펜 끝 압력 처리, 입력 → 화면 지연 | UI 툴킷, 도크·대화상자 구현 |

즉 **기하와 수식은 그대로 옮기고, 그것을 픽셀로 바꾸는 방법은 더 낫게 새로 만든다.** 왼쪽 넷째 행의 8비트 합성 누적은 구현처럼 보이지만 에어브러시 느낌을 좌우하므로 의도적으로 지킨다(§3.5).

---

## 1. 현재 아키텍처

### 1.1 규모 (헤더 포함 줄 수)

| 모듈 | 파일 | 줄 | 책임 |
|---|---|---|---|
| `src/document` (+`history/`) | 34 | 9,879 | 모델, 변경 관문 `DocumentController`, delta 히스토리, 선택·마스크 연산, 텍스트 → 획 |
| `src/render` (+`engine/`) | 47 | 9,812 | 우글거림 모션, 획 준비·래스터, 연산 재생, 레이어 계층 합성, 증분·표시 배율 렌더, 캐시 |
| `src/io` (+`serializer/`) | 31 | 9,159 | `.ugu` 코덱·검증·자산표·바이트 계획·bounded inflate, GIF/WebP, Wawa v10, 클립보드 |
| `src/app` | 19 | 1,790 | 복구, 인스턴스 락, 로그, 업데이트(Velopack/Sparkle), 메모리 예산 |
| `src/brush`, `src/input` | 6 | 595 | 브러시 프리셋, 1€ 필터 |
| `src/ui` | 129 | 26,887 | MainWindow(4.0K), CanvasWidget 계열(8.4K), 도크·대화상자, QRhi 표시 창 |
| `src/wasm` | 10 | 3,102 | 웹용 C ABI(`ugu_*` 116개, ABI 버전 9) |
| `src/main.cpp` | 1 | 316 | 시작 시퀀스, WinTab 활성화, 설정 이전 |
| `tests` | 56 | 32,123 | QTest 12 스위트(엔진 약 345 슬롯, UI 약 247 슬롯), 퍼저 4종 |
| `web/src` | — | 11,742 | Svelte 셸 + 워커. 엔진은 위 C++를 Qt for WebAssembly로 빌드한 것 |

### 1.2 데이터 흐름과 스레딩 (현재)

```
main.cpp ─▶ MainWindow (액션·메뉴·도크, 저장/복구/자동저장/내보내기 조율)
              ├─ CanvasWidget ── 입력(마우스·태블릿·터치) → StrokeStabilizer
              │      │             → IncrementalStrokeRenderer(타일 패치)
              │      │             → LayerSplitFrame 합성 → CanvasDisplayWindow(QRhi) 또는 software paint
              │      └─ 워커: 프레임 warmup(≤8), 상호작용 프레임(1), 도구 참조(1), 선택 가시성, 썸네일
              ▼
       DocumentController ── 후보 Document → prepare(검증·바이트 계획, io/) → DocumentDelta
              │                → DocumentUndoStack(64개 + 192 MiB) → 신호(documentChanged 등)
              ▼
       RenderEngine(정적 facade) ── LayerCompositionPlan → 계층 합성 → (StaticLayerCache)
                                     → LayerOperationReplay / DisplayScaleReplay → StrokeRenderer(QPainter)
```

- 가변 상태는 GUI 스레드가 소유하고, 워커에는 `shared_ptr<const Document>`와 취소 토큰만 넘긴다. 결과는 `QFutureWatcher` + generation 비교로 받는다.
- 프로세스 전역 가변 상태는 mutex로 보호되는 두 캐시(`StaticLayerCache`, `RasterAssetCache`)뿐이다.
- 표시는 `createWindowContainer`로 끼운 별도 `QWindow`(자체 스왑체인, 입력 투명, 이벤트 포워딩)다. 셰이더는 두 개뿐이다. `canvas_frame`은 CPU가 합성한 premultiplied 프레임을 체커보드 위에 얹고, `canvas_overlay`는 CPU `QPainter`로 그린 오버레이 텍스처를 덮는다. 프레임 텍스처는 `QImage::cacheKey` 동일성으로 판단한 더티 영역만 올린다. Linux에서는 RHI를 만들지 않아 software 경로로 떨어진다.

### 1.3 Qt 결합도 지도

Qt 식별자 사용 횟수(상위)와 결합의 성격이다.

| 모듈 | 주요 사용 | 결합의 성격 | 새 설계에서 |
|---|---|---|---|
| document | QUuid 240, QImage 190, QVector 146, QString 84, QRect 67, QTransform 59, QPainter 17, QObject | 값 타입 + 암묵 공유(COW 히스토리, 메모리 회계) + 신호. 텍스트 → 획에서 `QPainterPath::addText`와 Qt 곡선 평탄화 | 모델 재설계, COW는 `Arc`로 |
| render | QImage 336, QPainter 91, QPainterPath 35, QRgb 51, QMutex·QWaitCondition, QRadialGradient | 픽셀이 Qt 래스터 엔진으로 만들어진다 | 기하·수식은 이식, 래스터는 새로 |
| io | QString 284, QJson* 약 200, QByteArray 125, QImage 128, QCryptographicHash, qCompress, QSaveFile | 디스크 바이트와 열기 허용 판단이 Qt 직렬화 동작에 묶여 있다 | 새 형식으로 대체, 대부분 폐기 |
| app | QSaveFile, QLockFile, QStandardPaths, QSettings, QFutureWatcher, QMessageBox | OS 서비스. 일부는 UI를 직접 띄운다 | 세션 계층과 플랫폼 계층으로 |
| brush, input | QString, `QCoreApplication::translate`, QPointF | 거의 없음 | 그대로 이식 |
| ui | 전부: QWidget 계열 전반, QRhi, QTabletEvent + WinTab(GuiPrivate), QNativeGestureEvent, QAction 58개, QSettings(64회, 약 45개 고정 키 + 동적 키) | 전면 | egui로 재작성 |
| wasm | Qt for WebAssembly + 같은 엔진 | 엔진 재사용 | Rust wasm으로 교체 |

### 1.4 레거시를 털어내면서 사라지는 결합

C++ `.ugu` 호환을 요구하지 않으므로 아래는 **옮기지 않는다.** 분량으로 보면 직렬화 계층 약 6.0K줄(`src/io/serializer` 4.8K + `DocumentSerializer` 1.2K, 그중 바이트 계획 1.1K)과 Qt 래스터 재현 전체다.

- **Qt 직렬화 동작 재현**: `QJsonDocument` 출력 형식, `base64(qCompress)`, 관대한 base64·색·UUID 파서, 파일을 열 때 다시 직렬화해 128 MiB 이하인지 보는 판단, `QImage` 행 패딩 크기로 세는 마스크 예산, schema 1–13 버전 분기, schema 3 인라인 마스크.
- **바이트 계획**(`PreparedPlanBuilder`): 쓰기 결과의 정확한 길이를 미리 계산하는 장치. 새 설계에서는 직렬화 길이가 아니라 디코드된 메모리 크기로 한도를 판정한다.
- **Qt 래스터 엔진의 비트 단위 재현**과 그에 따른 라이선스 검토.
- **OS별 수학 차이 보존**: Classic 모션이 OS libm을 써서 v1.0.0 골든이 Windows와 macOS에서 다르다(`LegacyRenderGoldenTests.cpp:77-105`). 새 판은 결정적 수학으로 모든 OS와 wasm에서 같은 픽셀을 낸다(T-07이 구조적으로 해소된다).
- **C++ 판과의 상호 운용**: 복구 파일 교차 복구, Qt가 OS별로 등록하는 클립보드 형식 이름, QSettings 이전.

### 1.5 새 설계에서도 남는 결합 (옮길 때 주의)

1. **느낌을 정하는 기하와 수식.** `DeterministicNoise`(splitmix64 해시 + smoothstep 보간), `MotionTimeModel`, `ClassicStrokeMotion`(상수와 채널 값이 v1 호환 불변량), `StrokeMotionModel`, `BrokenLineModel`, `StrokeRenderer::prepare`(리샘플 간격 `clamp(width*0.55, 2, 5)`, 필압 함수 `pressureScale`, `pressureWidth`), 에어브러시 경도 → 그라디언트 정지점, 스프레이 입자 수·각도·반지름 분포, `colorWithOpacity`의 알파 반올림. 이 수식들은 그대로 옮긴다.
2. **선 획의 모양.** 지금은 quad 곡선 경로(`smoothedPath`: 점 사이 중점을 잇는 quad)를 `QPen` round cap·join(square 팁은 square cap·miter join)으로 그린다. 압력이 일정하면 경로 전체를 한 번에, 압력이 변하면 조각마다 굵기를 달리해 그린다. 새 래스터라이저는 같은 중심선과 굵기를 쓰되 커버리지 계산을 새로 한다.
3. **순서 있는 연산 재생 모델.** 레이어는 획뿐 아니라 픽셀 선택 변형, reframe(캔버스 자르기·크기 변경), 이미지 삽입, 합성 경계를 순서대로 가진다. 획이 프레임마다 흔들리므로 픽셀을 옮기는 연산도 프레임마다 다시 재생해야 한다. 이것은 레거시가 아니라 제품의 본질이라 유지하고, 표현만 정리한다(§3.1 모델).
4. **암묵 공유가 설계의 일부다.** COW delta 히스토리, 백킹 주소 기반 메모리 회계, `cacheKey`를 키로 쓰는 캐시, 도구 참조 캐시, `StaticLayerCache`의 소스 동일성 검사. `Arc` + 포인터 동일성 + 명시적 이미지 id로 바꾼다.
5. **`QObject` 신호가 도메인 이벤트 버스다.** `DocumentController` 신호 약 15개를 캔버스와 도크가 구독한다. 커밋이 반환하는 이벤트 목록으로 바꾼다.
6. **번역이 코어까지 들어가 있다.** `tr(` 약 848곳 중 io 180, document 66곳이다. 새 코어는 구조화된 오류를 반환하고 UI가 번역한다.
7. **비결정 입력.** 획 seed는 `QRandomGenerator::global()`, id는 `QUuid::createUuid()`다. 새 코어는 seed와 id 공급자를 주입받아 테스트에서 재현할 수 있게 한다.

### 1.6 그대로 가져갈 자산

- ANALYSIS_REPORT §10의 "유지할 설계": 트랜잭션 커밋, 리비전과 히스토리 노드 분리, delta 히스토리와 COW, 불변 스냅샷 스레딩, 결정적 우글거림, 캐시의 소스 동일성 검사, "증분 렌더 = 전체 렌더" 원칙, bounded inflate, 원자적 저장.
- 순수 함수에 가까운 UI 로직: 뷰포트 수학(`CanvasViewport.hpp`, `documentTransform`·`fitZoom`·`mapToDocument`), 제스처 수학, 획 샘플링 규칙(압력 클램프, 0.75px 간격, 끝점 압력 유지), `ShortcutBinding` 규칙, 도구 참조 문서 유도, 커서 선택, 미리보기 스케줄링 로직.
- 웹 C ABI(`BridgeDocument`)는 이미 "편집 세션" 단위 API다. Rust 세션 계층 API의 설계 원형으로 쓴다(함수 목록을 그대로 따를 필요는 없다).
- 웹 셸(`web/src/lib`)은 Qt 없이 같은 동작을 다시 표현한 선례다. 엔진만 바꾸면 계속 쓸 수 있고, egui 구현의 참고 자료도 된다.
- 테스트 스위트는 C++ 결과를 오라클로 쓰지 않더라도 **행동 명세**로 쓴다(어떤 편집이 거절돼야 하는지, undo가 어떻게 동작해야 하는지 등).

---

## 2. 원칙

1. **느낌은 그대로, 나머지는 더 낫게.** 우글거림 변위와 획 기하, 필압 응답, 합성 누적 특성은 같은 수식으로 옮기고 지표와 사람의 눈으로 확인한다. 파일 형식, 래스터 방법, 내부 구조는 유지보수성과 안정성 기준으로 새로 설계한다.
2. **C++는 느낌 비교가 끝날 때까지만 참조 도구다.** 비교에 필요한 내보내기 도구만 C++ 쪽에 만든다. 느낌 확인이 끝나면 Rust 결과를 동결 골든으로 삼고 C++ 참조는 버린다.
3. **main은 언제나 출시 가능하다.** Rust 작업은 C++ 데스크톱의 빌드·테스트·릴리스를 바꾸지 않는다.
4. **측정한 뒤 결정한다.** 래스터라이저, 이벤트 루프, 펜 입력 백엔드, 파일 형식 구조, 지연 예산은 스파이크 측정으로 정한다. ms·MB 수치는 측정 없이 쓰지 않는다.
5. **"미리보기 = 최종" 계약은 처음부터 구조로 보장한다.** 증분 렌더가 전체 렌더와 같아지도록 래스터라이저를 설계하고(§3.5), 이 계약을 1급 테스트로 둔다.
6. **모든 플랫폼에서 같은 픽셀.** Windows·macOS·Linux·wasm이 같은 문서에서 같은 픽셀을 낸다. 골든은 하나다.
7. **형식은 처음부터 버전과 마이그레이션을 갖는다.** 지금 `.ugu`가 겪은 문제(암묵적 호환 정책, 읽기 코드 곳곳의 버전 분기, 빈약한 fixture, A-05·T-06)를 새 형식 1판부터 막는다.

---

## 3. 권장 아키텍처

### 3.1 크레이트 구성

```
crates/
  ugu-base      정수 사각형·이미지 버퍼(premultiplied RGBA8)·마스크·id·오류 타입·결정적 수학
  ugu-model     Document/Layer/Op, 한도, 불변식 검증 (kurbo 기하 사용)
  ugu-motion    결정적 노이즈, 모션 3종, 끊어진 선, 획 준비(리샘플·변위), 1€ 필터
  ugu-raster    해석적 브러시 래스터(캡슐·dab), 다각형 채우기, 8비트 합성, 클립
  ugu-render    연산 재생, 레이어 계층 합성, 타일 렌더, 캐시, 표시 배율 미리보기
  ugu-format    새 파일 형식(컨테이너, 매니페스트, 청크), 버전·마이그레이션, 프리셋·클립보드 형식
  ugu-edit      편집 함수, 트랜잭션, delta 히스토리, 선택 연산, 텍스트 → 획, 이미지 가져오기
  ugu-export    GIF·WebP·PNG/JPEG 인코딩, 내보내기 정책
  ugu-session   편집 세션: 도구 상태 기계, 획 입력, 선택·변형 세션, 미리보기 스케줄러,
                문서 세션(열기·저장·자동저장·복구·종료 정책)
  ugu-gpu       wgpu 캔버스 표시(프레임 텍스처, 체커, 오버레이)
  ugu-platform  펜 입력, 파일 열기 이벤트, 업데이트, 단일 인스턴스, 창 크롬
  ugu-ui        egui 셸: 패널·도크·대화상자·설정·단축키·i18n·테마
  ugurugu       데스크톱 실행 파일 (winit 루프)
  ugu-wasm      웹 워커용 바인딩 (wasm-bindgen)
  ugu-reference 개발 전용: C++ 참조 내보내기를 읽어 비교 (배포물에 들어가지 않음)
```

허용하는 직접 의존 (이 표에 없는 방향은 금지, 순환 불가):

| 크레이트 | 직접 의존할 수 있는 크레이트 |
|---|---|
| ugu-base | 없음 |
| ugu-model | base |
| ugu-motion | model, base |
| ugu-raster | base |
| ugu-render | motion, raster, model, base |
| ugu-format | model, base |
| ugu-edit | render, motion, model, base |
| ugu-export | render, model, base |
| ugu-session | edit, export, format, render, motion, model, base |
| ugu-gpu, ugu-platform | base |
| ugu-ui | session, gpu, platform, base |
| ugurugu | ui, session, gpu, platform |
| ugu-wasm | session (스레드·파일 시스템 feature 끔) |
| ugu-reference | 전부 (개발 전용) |

- 지금의 document ↔ io ↔ render 순환(A-01)은 이 표로 불가능해진다. 편집 커밋이 직렬화 계층에 묻던 것(바이트 계획)은 사라지고, 한도 판정은 model의 메모리 기준 함수 하나로 모인다.
- `ugu-session` 아래의 크레이트는 모두 `wasm32`에서 빌드된다. 파일 시스템과 OS 스레드는 session 이상에서만 쓰고, 렌더 병렬화는 feature로 끈다.
- 전역 static 캐시를 두지 않는다. `RenderContext { static_layers, raster_assets }`를 명시적으로 넘긴다.
- `#![forbid(unsafe_code)]`가 기본이다. 예외는 `ugu-raster`의 측정으로 필요가 확인된 픽셀 루프, `ugu-platform`의 OS FFI, `ugu-wasm` 경계다.

**모델 재설계** (A-06 반영):

```rust
struct Layer { id, name, kind: LayerKind, parent: Option<LayerId>, visible, reference,
               opacity, blend: BlendMode, clip_to_below, wobble: Option<WobbleSettings>,
               initial_canvas: IntSize, ops: Arc<[Op]> }
enum Op {
    Paint(StrokeOp), Erase(StrokeOp), Fill(FillOp),
    PixelTransform(PixelTransformOp), Reframe(ReframeOp), Image(ImageOp),
    SectionBoundary,
}
struct StrokeOp { id, seed: u64, color: Srgba8, width: f64, brush: BrushSettings,
                  points: Arc<[StrokePoint]>, clip: Option<StrokeClip> }
struct WobbleSettings { amount: f64, motion: MotionSettings }   // 레이어 override는 한 쌍으로만
```

- 표현 가능한 상태 = 유효한 상태에 가깝게 만든다. 지금 `Stroke`는 mode 하나와 optional 6개로 무효 조합을 4곳에서 따로 검사한다.
- 순서 있는 연산 재생의 의미(캔버스 시대 epoch, 섹션 경계에서 평탄화, 지우개가 자기 섹션에만 닿음)는 그대로 유지한다. 느낌과 결과물에 직접 영향을 주기 때문이다.

### 3.2 계층 경계 규칙

| 계층 | 크레이트 | 허용 | 금지 | 테스트 방식 |
|---|---|---|---|---|
| core | base, model, motion, edit | 순수 계산. seed와 id는 공급자를 주입받는다 | 파일, 스레드, 시계, 전역 상태, UI 문자열 | 단위 테스트, 속성 테스트, 행동 명세 |
| renderer | raster, render | `Arc<Document>` 스냅샷 + 프레임 + 출력 크기 + 취소 토큰 → 이미지. 명시적 캐시 객체 | GPU, UI 타입, 문서 변경 | 골든, 내부 오라클, 느낌 지표 |
| IO | format, export | 바이트 ↔ 모델, 이미지 → 바이트 | 경로·파일 열기(세션 담당) | 형식 골든, 왕복, 퍼저 |
| UI | session / gpu / ui / platform | session: 창 없이 테스트하는 상호작용 로직. gpu: 표시만. ui: egui 위젯만. platform: OS FFI만 | ui가 render·edit를 직접 호출(반드시 session을 거침) | session은 합성 입력 시나리오, ui는 egui_kittest 스냅샷과 체크리스트 |

코어는 번역된 문자열 대신 `enum` 오류(예: `RejectReason::StrokeLimit { limit }`)를 반환하고 사용자 문구는 UI가 만든다. 커밋은 `Committed / Unchanged / Rejected(reason)` 세 가지만 반환한다(D07).

### 3.3 Qt 타입 대체표

| Qt | Rust | 주의 |
|---|---|---|
| `QImage` ARGB32_Premultiplied | `ugu_base::Image`: premultiplied RGBA8, `Arc<[u8]>` COW, 할당·변경 때 새 `ImageId`(cacheKey 대체) | wgpu `Rgba8Unorm`에 그대로 올라간다. 내부 표현으로 `image::RgbaImage`(straight, COW 없음)를 쓰지 않는다 |
| `QImage` Grayscale8, 1비트 packed | `Mask8`, `BitMask` | 임계값 128 규칙(14개 파일 약 47곳)을 `is_set()` 하나로 모은다(Q-06) |
| `QPainter` | `ugu-raster` 함수(§3.5) | save/restore 스택 대신 상태를 값으로 넘긴다 |
| `QPainterPath` | `kurbo::BezPath` | 평탄화·스트로크 확장은 kurbo 또는 자체 |
| `QPointF`/`QRectF`/`QSizeF`/`QTransform` | `kurbo::Point`/`Rect`/`Size`/`Affine` | 레거시 형식 호환이 없으므로 Qt 행벡터 규약을 따를 이유가 없다. 표준 아핀 하나로 통일 |
| `QPoint`/`QSize`/`QRect` | 자체 `IntPoint`/`IntSize`/`IntRect` (반열린 구간) | `QRect::right()`의 포함 구간 관례를 버린다 |
| `qreal`, `qRound`, `qFuzzyCompare` | `f64`, 명시적 반올림 함수 하나 | 느낌 수식(알파 반올림, 입자 수)에 쓰이는 반올림 규칙만 일관되게 |
| `std::hypot`·`sin`·`cos` 등 | `libm` 크레이트(결정적 구현) | OS libm을 쓰지 않아야 모든 OS·wasm에서 같은 픽셀. FMA는 명시적으로만 |
| `QUuid` | `uuid::Uuid` (v4, 테스트에서는 주입된 생성기) | |
| `QString` | `String` (UTF-8) | 길이 제한은 문자(스칼라) 단위로 새로 정의 |
| `QVector`/`QList` | `Vec<T>`, 히스토리에서 공유되는 것은 `Arc<[T]>` | COW 히스토리와 메모리 회계의 핵심 |
| `QHash`/`QSet`/`QMap` | `HashMap`/`HashSet`/`BTreeMap` | 출력 순서가 필요한 곳은 BTreeMap |
| `QByteArray` | `Vec<u8>` / `bytes::Bytes` | |
| `QJsonDocument`, `qCompress`, base64 | 새 형식(§3.4). `serde`/`serde_json`(매니페스트), `zip`, 순수 Rust deflate | wasm에서도 C 의존 없이 빌드 |
| `QCryptographicHash` | `sha2` | 자산 콘텐츠 주소 |
| `QSaveFile` | 같은 폴더 임시 파일 → flush·fsync → 교체(Windows `ReplaceFileW`), 취소 시 정리 | |
| `QLockFile` | `fd-lock`/`fs4` + 소유 프로세스 생존 판정 | |
| `QStandardPaths` | `directories` | 새 앱 정체성의 경로 |
| `QSettings` | 타입 있는 설정 파일(serde) | 문자열로 값이 돌아오는 문제(`.wwpreset` 의심 결함)가 구조적으로 사라진다 |
| `QThreadPool`/`QtConcurrent`/`QFutureWatcher` | `rayon`(타일·프레임 병렬) + 전용 워커 스레드 + 채널, UI 루프가 매 프레임 drain 후 `request_repaint` | generation·취소 토큰 패턴은 그대로 |
| `QMutex`/`QWaitCondition` | `parking_lot`, 대기 슬롯은 Drop 가드 | B-02 유형을 타입 수준에서 막는다 |
| `QObject` 신호 | `CommitOutcome { revision, events: Vec<DocEvent> }` | 관찰자 그래프 없음 |
| `QTranslator`/`.ts` | Fluent(`.ftl`), ko/ja 728개 메시지 변환 스크립트 | 번역 완성도 CI 게이트 유지 |
| `QAction`/`QKeySequence` | `Command` enum + 키 바인딩 맵(ShortcutBinding 규칙 이식) | objectName 문자열 조회(A-04) 제거 |
| `QImageReader`/`QImageWriter` | `image` 크레이트(디코드 한도, EXIF 회전), PNG·JPEG 인코더, WebP는 libwebp 바인딩 | 포맷 플러그인 누락(R-01)이 구조적으로 사라진다 |
| `QFont`, `QPainterPath::addText` | `fontdb` + `skrifa` + `rustybuzz`(또는 `parley`) | 텍스트는 생성 순간 획으로 굳는다. 글리프 윤곽은 조금 달라질 수 있다 |
| `QRhi` | `wgpu` | 셰이더 2쌍(GLSL, 각 30줄 미만) → WGSL |
| `QTabletEvent` + WinTab(GuiPrivate) | `ugu-platform::tablet` | §3.7 |
| `QClipboard`/`QMimeData` | 플랫폼별 구현(앱 전용 형식 + 이미지 동시 기록) | `arboard`는 사용자 정의 형식을 다루지 않는다 |
| `QFileDialog`/`QMessageBox`/`QInputDialog`/`QColorDialog` | `rfd` / egui 모달 | |

### 3.4 새 파일 형식

레거시 호환이 없으므로 지금 형식의 약점(base64로 33% 불어나는 블롭, 128 MiB 텍스트 파싱, 직렬화 길이에 묶인 판단, 암묵적 버전 분기)을 없앤 형식을 새로 만든다. 0단계 스파이크에서 크기·속도를 측정해 확정한다.

- **권장 구조: ZIP 컨테이너**(Krita `.kra`, OpenRaster와 같은 방식)
  - `manifest.json`: 형식 id, 형식 버전, 읽는 데 필요한 최소 버전, 문서 설정, 레이어 트리, 연산 목록(획은 청크 참조)
  - `ops/<layer-id>.bin`: 획 점 배열 등 큰 수치 데이터. 명시적 헤더(매직, 버전, 엔디언, 개수)가 있는 리틀엔디언 배열. 점은 f64로 저장해 저장 → 열기 → 렌더가 비트 단위로 같게 한다
  - `masks/<sha256>.bin`, `assets/<sha256>.png`: 콘텐츠 주소 블롭
  - `thumbnail.png`: 파일 탐색기·최근 파일용
- **원칙**
  - Rust 구조체를 그대로 덤프하는 형식(bincode 등)을 쓰지 않는다. 필드를 바꾸면 조용히 깨지기 때문이다. 매니페스트는 명시적 스키마, 바이너리 청크는 명시적 헤더를 가진다.
  - 버전마다 `migrate_vN_to_vN+1` 함수 하나, 버전마다 골든 fixture 하나를 저장소에 둔다.
  - 더 새로운 파일은 "이 파일은 형식 N, 이 앱은 M까지 읽음"으로 거절한다.
  - 한도는 디코드된 메모리 크기로 판정하고, 압축 해제는 출력 상한을 둔다(지금의 bounded inflate 원칙 유지).
  - 순수 Rust deflate로 wasm에서도 같은 코드가 돈다.
- **함께 정리할 것**: 자동저장·복구 파일도 같은 형식, 클립보드 앱 전용 형식은 단일 레이어 문서, `.wwpreset`은 타입 있는 새 프리셋 형식.
- **확장자**: 새 확장자를 쓴다(이름은 결정 항목). 같은 `.ugu`를 쓰면 기존 사용자의 파일이 새 앱에 연결돼 "열 수 없는 파일"이 생긴다.

### 3.5 렌더러: CPU 정답 + 새 래스터라이저

**CPU 렌더러를 정답으로 둔다.** 근거는 세 가지다.

1. 저장·내보내기 픽셀은 기기와 OS에 관계없이 같아야 한다. GPU 래스터는 벤더·드라이버마다 결과가 다르다.
2. CI와 웹에는 GPU가 보장되지 않는다. 엔진 테스트는 헤드리스로 돌아야 한다.
3. 지금도 표시만 GPU가 하고 픽셀은 CPU가 만든다. wgpu 이식 범위가 작다.

**래스터라이저는 이 앱의 브러시에 맞춰 새로 만든다.** 권장 설계는 다음과 같다.

- **해석적 커버리지, 타일 단위**
  - round 팁 선: 리샘플된 점 사이를 캡슐(양 끝 굵기가 다르면 round cone)로 보고 픽셀 중심까지의 거리로 커버리지를 계산한다. 한 획의 커버리지는 캡슐들의 합집합(최댓값)이고, 합성은 획당 한 번이다.
  - square 팁 선: 같은 중심선을 square cap·miter join 윤곽으로 확장해(kurbo 스트로크 확장 또는 자체) 다각형으로 채운다.
  - 점·하드 dab·스프레이 입자: 원·사각형의 해석적 커버리지. 소프트 에어브러시 dab: 경도 정지점을 가진 방사형 감쇠 함수. dab은 지금처럼 하나씩 합성해 누적시킨다.
  - 일반 다각형(채운 텍스트, 올가미, 사각형·타원 선택): nonzero·even-odd 스캔 변환기.
  - 비AA 모드(기본값): 픽셀 중심이 안에 있으면 1, 아니면 0. 지금의 또렷한 가장자리 느낌을 유지한다. AA 모드: 해석적 면적 커버리지.
- **이 설계를 권장하는 이유**
  - **증분 렌더 = 전체 렌더가 구조적으로 성립한다.** 타일의 결과가 그 타일에 닿는 원시들에만 의존하고 합집합은 순서와 무관하므로, 그리는 중 타일 패치와 전체 렌더가 저절로 같아진다. 지금 이 계약을 지키려고 둔 복잡한 코드(`paintsLineInPieces`, `repaintIsIdempotent`, 체크포인트, 조각 단위 재그리기 등. `StrokeRenderer`·`IncrementalStrokeRenderer`·`StrokeCoverageRenderer` 합계 3.2K줄)가 크게 줄어든다.
  - 타일 단위라 rayon으로 병렬화되고, 큰 캔버스의 메모리 사용이 타일 크기로 묶이며, 같은 수식을 나중에 GPU 미리보기로 옮기기도 쉽다.
  - 이 앱의 원시는 몇 가지뿐이라 범용 2D 엔진보다 작고 통제 가능하다. 결정적이고 wasm에서도 같다.
- **대안**: `tiny-skia`(Skia 계열 CPU 래스터) 같은 범용 크레이트. 0단계 S1에서 두 방식을 같은 장면으로 비교한다(느낌 지표, 성능, 증분 일치, 코드량).
- **합성은 8비트 premultiplied 정수식을 유지한다.** 저불투명 에어브러시 dab이 쌓이는 모양(어디서 포화되는지, 줄무늬가 생기는지)은 8비트 반올림에 좌우된다. 16비트나 실수 합성으로 바꾸면 에어브러시 느낌이 달라지므로, Qt의 SourceOver·DestinationOut·Multiply·Screen·Overlay와 같은 계열의 정수식을 쓰고 S1에서 누적 곡선을 C++와 비교한다. 이것은 비트 호환이 아니라 느낌 호환을 위한 선택이다.
- **픽셀을 옮기는 연산**(픽셀 선택 변형, reframe)은 지역성이 깨지므로, 그 지점에서 레이어를 한 번 통째로 확정하고 그 뒤부터 다시 타일 단위로 계속한다. 지금 이런 연산을 만나면 전체 캔버스 렌더로 돌아가는 것과 같은 원리다.
- **표시 배율 미리보기**(축소 표시에서 직접 낮은 해상도로 재생)는 근사로 남긴다. 단 pen-up에 승격되는 프레임은 반드시 정답 렌더와 같아야 한다.

### 3.6 egui와 캔버스 연결

```
winit 이벤트 ──┬─▶ egui-winit ─▶ egui 입력 (패널·도크·대화상자)
               └─▶ InputRouter ── 캔버스 위이고 egui가 포인터를 원하지 않을 때 ──┐
플랫폼 펜 훅 ─▶ PenSample 큐 (압력·지우개·근접·ms 타임스탬프) ───────────────────┤
                                                                               ▼
                                                    ugu-session (도구 상태 기계, 1€, 증분 렌더)
                                                               │  워커 결과 채널
                                                               ▼
egui 프레임: 레이아웃 → 캔버스 rect 확보 → egui_wgpu::Callback(CanvasPaint) → 오버레이 Shape
egui-wgpu 렌더 패스: Callback.prepare(더티 영역 write_texture, 유니폼) → paint(체커 + 프레임 쿼드) → 오버레이 → 위젯
```

- **직접 winit 루프 + egui-winit + egui-wgpu를 권장한다(eframe 대신).** (1) 1€ 필터가 지금처럼 동작하려면 펜 샘플을 egui의 프레임 단위 집계 전에 원본 타임스탬프와 함께 받아야 한다. (2) 지연 수정(D26 등)의 성과를 유지하려면 present mode와 프레임 지연(`desired_maximum_frame_latency`)을 직접 제어해야 한다. (3) WinTab과 macOS 이벤트 모니터를 창 핸들에 붙이기 쉽다. eframe은 0단계 프로토타입에만 쓴다.
- **캔버스 그리기**: `egui_wgpu::CallbackTrait`를 구현한다. `prepare`에서 더티 영역만 `queue.write_texture`로 올리고, `paint`에서 egui 렌더 패스 안에 캔버스 쿼드를 그린다. 표면이 하나라 지금처럼 별도 네이티브 창과 입력 포워딩을 둘 필요가 없다.
- **더티 추적**: `QImage::cacheKey` 대신 세션이 "표시 프레임 id + 누적 더티 영역"을 명시적으로 관리한다.
- **오버레이**: 그림자, 테두리, 개미행진(흰 1.8px + 어두운 1px 4/4 점선, 120ms 이동), 브러시 링(어두운 3px + 밝은 1px, 지우개는 점선), 텍스트 배치 미리보기를 egui `Shape`(벡터)로 그린다. 지금처럼 오버레이 텍스처 전체를 CPU로 다시 그리지 않아도 된다. 브러시 링이 멈추는 류의 회귀(D26)는 세션 계층 테스트로 막는다.
- **재그리기 정책**: egui 반응형 모드. 입력, 워커 결과, 재생 타이머가 있을 때만 다시 그린다. egui 전체 레이아웃 비용은 0단계에서 측정한다.
- **도크**: `egui_dock`(탭·분할)을 우선 검토한다. OS 독립 플로팅 창은 범위 결정 항목이다.
- **IME**: 텍스트 도구와 레이어 이름에 한국어·일본어 입력이 필요하다. 0단계 S2에서 확인한다.

### 3.7 플랫폼 계층

- **펜 입력 (UI의 최대 위험)**
  - 지금은 Qt 비공개 API로 WinTab을 켜고(`main.cpp:43-73`), 압력·지우개 끝·근접 이탈·ms 타임스탬프를 쓴다. 기울기·회전은 쓰지 않는다.
  - winit은 0.31 계열(베타)에서 Windows·Wayland·Web 펜 입력을 Pointer 이벤트로 추가했지만 macOS 펜 필압은 들어 있지 않다. egui-winit이 따르는 winit 버전과도 맞춰야 한다.
  - 자체 추상을 둔다: `PenSample { pos, pressure, device: Pen | Eraser | Mouse | Touch, phase, timestamp_ms }`.
    - Windows: WM_POINTER(Windows Ink) + WinTab(기본값은 측정과 실제 펜 확인으로 결정).
    - macOS: objc2로 NSEvent 로컬 모니터를 달아 tabletPoint·tabletProximity와 마우스 이벤트의 pressure·pointingDeviceType를 읽는다.
    - Linux: Wayland tablet(winit), X11은 best effort.
  - 지금의 세부 규칙(릴리스 압력 0이면 마지막 압력 사용, 지우개 끝이면 도구와 무관하게 지우개, 펜이 터치를 억제, 측면 버튼 무시, 근접 이탈 시 취소하되 눌린 키 유지)은 세션 계층의 순수 로직으로 옮기고 합성 샘플로 테스트한다(T-04).
- **제스처**: 두 손가락 이동·확대·회전(터치), macOS 트랙패드 pinch·rotate.
- **macOS 파일 열기**: winit에 경로가 없다. objc2로 Apple Event(open documents) 처리기를 달고, Info.plist에 새 확장자의 문서 타입을 선언한다.
- **업데이트**: Windows는 Velopack 공식 Rust 크레이트, macOS는 Sparkle 유지(objc2로 `SPUStandardUpdaterController` 호출, 서명·공증·appcast 파이프라인 재사용), Linux는 Velopack(AppImage)을 검토한다. 새 앱 정체성이면 피드도 새로 만든다.
- **단일 인스턴스·복구·로그**: 새 앱 정체성의 경로를 쓴다. C++ 판과의 교차 복구는 하지 않는다.

### 3.8 웹

- Qt for WebAssembly와 emsdk를 걷어내고, 엔진은 `ugu-wasm`(wasm-bindgen)으로 바꾼다. 워커 구조(`EngineClient` → Worker → 엔진)는 유지한다.
- **웹 UI는 Svelte 셸을 유지할 것을 권장한다.** 이미 동작하고 브라우저 시나리오 26개가 있으며, 파일 처리·브라우저 저장소·웹 입력이 웹에 맞게 되어 있다. egui로 웹 UI까지 통일하면 UI 코드가 하나로 줄지만, 웹에서의 펜 필압·IME·접근성·wasm 크기 위험이 있다. 데스크톱 출시 뒤 스파이크로 다시 판단한다.
- 엔진 경계는 지금의 함수 116개를 그대로 따르지 않고 `ugu-session` API에 맞춰 다시 설계한다. 단 워커 메시지의 응답 모양(더티 영역 픽셀, 레이어 목록, 선택 윤곽, undo 가능 여부)은 셸 변경을 줄이도록 최대한 유지한다.
- 웹의 자동복구 저장소에 남은 이전 형식 초안은 새 엔진에서 열리지 않는다. 전환 전에 사용자 안내가 필요하다(결정 항목).

---

## 4. 검증 체계 — 비트 동일이 아니라 느낌 동일

### 4.1 무엇을 어떻게 비교하나

| 층 | 비교 대상 | 기준 | 방법 |
|---|---|---|---|
| M 모션 | 우글거림 변위: 획·프레임별 준비된 점 | 수치 일치(허용 오차 1e-9px 수준. OS libm 대신 결정적 수학을 쓰므로 비트 동일은 요구하지 않음) | C++ 참조 도구의 `geometry` 덤프와 Rust 결과 비교 |
| I 입력 | 1€ 필터 출력, 최소 간격·끝점 압력 처리 | 수치 일치 | 기록한 펜 입력(좌표·압력·타임스탬프)을 양쪽에 넣어 비교 |
| F 느낌 | 래스터 결과 | 지표 허용치(아래) | 장면 행렬을 양쪽에서 렌더해 지표 계산 |
| H 사람 | 그리는 느낌, 우글거리는 느낌 | 사용자 승인 | 나란히 재생하는 비교 도구로 확인 |
| R 내부 계약 | 증분 = 전체, 승격 프레임 = 정답 렌더, 저장 → 열기 → 렌더 동일, 모든 OS·wasm 동일 | 비트 동일 | Rust 자체 골든과 속성 테스트 |
| B 행동 | 편집 거절 조건, undo/redo, 선택·변형 결과 | 명세 동일 | 기존 C++ 테스트를 명세로 읽어 Rust 테스트로 다시 씀 |

**F층 지표** (장면마다, 프레임마다)

- 커버리지 IoU: 알파 > 0 영역(비AA) 또는 알파 가중 IoU(AA).
- 가장자리 편차: 두 결과의 윤곽선 사이 거리의 평균·최대(px).
- 평균 알파 차: 획이 닿는 영역의 합집합 안에서.
- 누적 곡선: 같은 위치에 에어브러시 dab을 k개 쌓았을 때의 알파 값 수열.
- 반경 프로파일: 소프트 dab의 중심에서 바깥으로의 알파 곡선.
- 움직임: 프레임마다 각 획 커버리지의 무게중심 이동량 수열(흔들림의 크기와 리듬).

**허용치를 정하는 방법**: 지금 C++ 앱은 Classic 애니메이션 프레임을 Windows와 macOS에서 서로 다르게 그리고, 그 차이는 사용자에게 문제가 된 적이 없다. 0단계에서 같은 장면을 C++ Windows판과 macOS판으로 렌더해 위 지표를 재고, 그 분포를 "알아챌 수 없는 차이"의 기준선으로 삼는다. Rust 결과가 그 범위 안이면 F층 통과로 본다. 범위를 넘는 장면은 H층(사람 확인)으로 넘긴다.

### 4.2 참조 도구와 장면 행렬

- **C++ 쪽** `tools/ReferenceExport.cpp`(`EngineDigestProbe` 확장, `EXCLUDE_FROM_ALL`, 개발용):
  - `scene <doc>`: 모든 기본값과 레이어 override를 풀어 쓴 중립 장면 JSON. Rust 개발 전용 크레이트 `ugu-reference`가 이것을 읽어 Rust 문서를 만든다. 제품은 C++ `.ugu`를 읽지 않는다.
  - `geometry <doc>`: 획·프레임별 준비된 점
  - `frames <doc> --frames all --size WxH`: 프레임 PNG
  - `stabilize <trace>`: 펜 입력 기록 → 1€ 필터 출력
  - `dabs <cases>`: 에어브러시·스프레이 원시 결과
- **seed·id 주입**: C++ 쪽에 seed·uuid 공급자 주입 지점을 추가해 같은 입력에서 같은 장면이 나오게 한다(작은 C++ 변경).
- **장면 행렬** (작은 캔버스, 저장소에 둠): 브러시 프리셋 17종 × 필압 프로파일(일정·증가·감소·떨림) × 모션 3종 × 흔들림 양 몇 단계 × AA 켬/끔, 그리고 레이어 기능(클립·그룹·블렌드 4종·섹션 경계·reframe·픽셀 선택 변형·이미지·frozen/packed fill·끊어진 선·레이어 override). 큰 문서(`kimcozo_service.ugu` 등)는 로컬에서만 쓴다(커밋 금지 규칙 유지).
- **비교 도구**: 같은 장면의 C++ 결과와 Rust 결과를 나란히(그리고 겹쳐서 차이 강조) 재생하는 뷰어. H층 확인과 F층 실패 분석에 쓴다.
- **느낌 확인이 끝나면**: Rust 결과를 동결 골든으로 커밋하고 C++ 참조 도구와 참조 이미지는 더 갱신하지 않는다. 이후 렌더를 의도적으로 바꾸면 골든 갱신과 함께 비교 도구로 이전 골든과 나란히 확인한다.

### 4.3 결정성

- 렌더 경로의 초월 함수는 `libm` 크레이트로, FMA는 명시적으로만 쓴다. 합산 순서가 바뀌는 병렬 누산(타일 결과의 순서 없는 합산 등)을 금지한다.
- CI에서 Windows·macOS·Linux·wasm(Node) 네 곳의 골든 digest가 같은지 검사한다.

### 4.4 기존 테스트 활용

| 기존 자산 | 활용 |
|---|---|
| 엔진 QTest 약 345 슬롯 | 오라클이 아니라 행동 명세로 읽는다. 스위트별로 "유지할 행동 / 형식 레거시라 버릴 행동"을 분류한 이식 대장을 만들고 Rust 테스트로 다시 쓴다. `DocumentSchema`·`SerializationBudget`·`RasterAssetTable`·`WawaV10Reader`·`LegacyRenderGolden`의 대부분은 버리는 쪽이다 |
| 정확성 오라클(증분 = 전체, 영역 = 전체의 일부, 정적 캐시 = 비캐시, 승격 프레임 = `render`) | Rust 내부 계약 테스트로 그대로 옮긴다 |
| `ImageDifference` 지표와 임계값 | 표시 배율 미리보기(근사)와 GPU 표시 대 CPU 표시(채널 ≤ 8) 비교에 그대로 쓴다 |
| UI QTest 약 247 슬롯 | 상호작용 로직(선택·변형 세션, 뷰포트, 입력 상태 기계, 미리보기 스케줄링, 복구 결정 행렬)은 세션 테스트로. 위젯 배치·Qt 전용은 egui 테스트나 체크리스트로 바꾸거나 버린다 |
| 웹 브라우저 시나리오 26개 | 새 엔진에 맞게 고쳐 웹 전환의 인수 테스트로 쓴다. 대부분은 행동 단언이라 그대로 유효하다 |
| 퍼저 4종 | 새 형식 읽기, 클립보드 형식, 프리셋 형식 퍼저로 다시 만든다 |
| `RenderBenchmark`, `StressDocumentGenerator` | Rust판을 만들고, 같은 장면에서 C++와 시간·메모리를 비교한다 |

---

## 5. 위험이 낮은 순서

| 순위 | 대상 | C++ 규모 | 오라클 | 위험 |
|---|---|---|---|---|
| 1 | 우글거림 수학: DeterministicNoise, MotionTimeModel, Classic/StrokeMotionModel, BrokenLineModel | 약 0.8K | M층 수치 비교 | 매우 낮음 |
| 2 | 1€ 필터, 획 샘플링 규칙, 브러시 프리셋 값 | 약 0.6K | I층 수치 비교 | 매우 낮음 |
| 3 | 새 모델 타입, 한도, 불변식 | 약 1.3K 참고 | 속성 테스트 | 낮음 |
| 4 | 획 준비(리샘플·변위 적용) | 렌더의 일부 | M층 | 낮음 |
| 5 | 새 파일 형식 | 새로 작성 | 형식 골든, 퍼저 | 낮음–중 (설계가 핵심) |
| 6 | 래스터라이저 | 새로 작성 | F·H층 | **높음** (느낌을 좌우) |
| 7 | 렌더 엔진: 연산 재생, 계층 합성, 캐시, 표시 배율 | 약 5K 참고 | F층, R층 | 중 |
| 8 | 편집·히스토리·선택 연산 | 약 8.3K 참고 | B층 | 중 |
| 9 | 내보내기(GIF·WebP·PNG/JPEG) | 약 1.9K | 픽셀·디코드 확인 | 낮음 |
| 10 | 세션: 도구 상태 기계, 미리보기 스케줄러, 문서 세션 | 약 8.4K + MainWindow 일부 | B층, 합성 입력 | 중–높음 |
| 11 | 웹 엔진 교체 | 브리지 3.1K + 셸 수정 | 브라우저 시나리오 | 중 |
| 12 | 데스크톱 셸(egui·wgpu·펜·IME·도크·업데이트) | 약 19K 재작성 | 체크리스트, 측정 | **높음** |

"위험이 낮다"와 "먼저 확인해야 한다"는 다르다. 6번(래스터라이저 느낌)과 12번(펜·지연)은 실패하면 계획을 바꾸는 항목이라, 구현은 뒤에 하더라도 0단계 스파이크에서 먼저 측정한다.

---

## 6. main을 계속 쓸 수 있게 유지하는 방법

- **저장소 배치**: 루트에 `Cargo.toml`(workspace)과 `crates/`를 추가해 CMake와 공존시킨다. `rust-toolchain.toml`로 툴체인을 고정하고 `Cargo.lock`을 커밋한다. `cargo-deny`로 의존성 라이선스(GPL-3.0-or-later와 호환)와 출처를 검사한다.
- **Qt 앱에 Rust를 끼우지 않는다.** 데이터 모델과 형식이 다르므로 FFI 접착 코드는 전부 버려질 코드다.
- **CI**: 새 잡을 추가하고 기존 C++ 잡과 `release.yml`은 그대로 둔다.
  - `rust-static`: rustfmt, clippy `-D warnings`, SPDX 헤더 검사에 `*.rs` 추가
  - `rust-test`: Windows·macOS·Linux + wasm(Node)에서 테스트, 골든 digest가 네 곳에서 같은지 검사
  - `reference`: C++ `ReferenceExport` 빌드 → 장면 행렬 지표 계산(느낌 확인이 끝나면 제거)
  - `quality` 게이트의 needs에 추가
- **래칫**: 느낌 지표 잡은 처음에는 보고만 하고, 장면별로 통과하면 차단 게이트로 바꾼다. "알려진 초과 장면 목록"은 줄어들기만 할 수 있다.
- **브랜치**: 단계 안의 작은 단위마다 짧은 브랜치를 만들고 main에 fast-forward로 합친다(지금 방식). 오래 사는 `rust` 브랜치는 만들지 않는다.
- **C++ 쪽 작업 규칙**
  - C++ 웹 엔진(`src/wasm`과 웹용 엔진 수정)은 지금부터 치명적 결함만 고친다. 진행 예정이던 W-03/R11(웹 디코드 예산) 같은 엔진 쪽 항목은 Rust 엔진의 설계 요구사항으로 옮긴다. Svelte 셸 쪽 수정은 셸이 남으므로 계속 의미가 있다.
  - C++ 데스크톱은 Rust 데스크톱 출시 때까지 버그 수정과 출시를 계속한다. 렌더 느낌을 바꾸는 C++ 변경은 참조 이미지를 다시 생성해야 하므로, 느낌 확인 기간에는 피한다. 세션 단계 착수부터는 기능 동결(버그 수정만).
- **출시**
  - 웹: Rust 엔진이 브라우저 시나리오를 통과하면 엔진을 교체한다. Rust 코어가 실제 사용자 환경에서 처음 도는 곳이 된다.
  - 데스크톱: 파일 형식이 다르므로 Rust 데스크톱은 **새 앱 정체성**(앱 id, 설치 위치, 확장자, 업데이트 피드)으로 낸다(확정). 같은 앱 id로 자동 업데이트하면 기존 사용자의 `.ugu`가 갑자기 열리지 않게 되기 때문이다. C++ 2.x는 그대로 설치해 둘 수 있고, 마지막 2.x 릴리스 뒤로는 새 기능을 넣지 않는다.
  - Qt·CMake·C++ 소스 삭제는 Rust 데스크톱 출시 뒤에 한다. 삭제 직전 커밋에 태그를 남긴다.
- **버전**: 지금 `CMakeLists.txt`의 `project(... VERSION)`을 CI·릴리스 4곳이 읽는다. Rust 판은 Cargo 버전을 쓰고, 두 판이 함께 있는 동안 CI 버전 검사는 판별로 나눈다.

---

## 7. 단계별 로드맵

각 단계의 완료 조건을 만족해야 그 단계에 의존하는 작업을 시작한다.

```
0 ─▶ 1 ─┬─▶ 2 ─────┬─▶ 4 ─▶ 5 ─┬─▶ 6 (웹 전환) ──────┐
        └─▶ 3 ─────┘           └─▶ 7 (데스크톱 셸) ─┴─▶ 8 (출시·Qt 제거)
0 ─▶ 7의 셸 골격 (가짜 세션으로 먼저, 5 뒤에 실제 연결)
```

### 0단계 — 기반, 참조 도구, 스파이크

- **목표**: 느낌을 재는 체계를 먼저 세우고, 계획을 바꿀 수 있는 위험을 측정한다.
- **범위**
  - Cargo 워크스페이스 골격, Rust CI 잡(보고 전용), `cargo-deny`.
  - C++ `ReferenceExport`와 seed·uuid 주입 지점, 장면 행렬, 느낌 지표 계산기, 나란히 비교 뷰어.
  - 허용치 기준선: 같은 장면을 C++ Windows판과 macOS판으로 렌더해 지표 분포를 잰다.
  - **S1 래스터라이저**: 해석적 타일 래스터(§3.5)와 `tiny-skia`로 장면 행렬의 핵심 장면(비AA·AA round 선, 필압 변화 선, square 선, 하드·소프트 dab 누적, 스프레이, 지우개)을 그려 느낌 지표, 증분 일치, 성능, 코드량을 비교한다.
  - **S2 셸**: 직접 winit 루프 + egui + wgpu로 4096² 캔버스, 더티 영역 업로드, 펜 입력(Windows Ink·WinTab, macOS NSEvent)을 띄우고, 입력에서 화면까지의 지연을 지금 앱과 같은 방법(SendInput + 화면 캡처)으로 비교한다. egui 레이아웃 비용과 한국어·일본어 IME를 확인한다.
  - **S3 파일 형식**: ZIP 컨테이너 시제품으로 큰 문서(스트레스 생성기 결과)의 저장·열기 시간과 파일 크기를 지금 `.ugu`와 비교한다.
- **완료 조건**
  - Rust CI 잡이 C++ 빌드·릴리스에 영향 없이 녹색.
  - 장면 행렬, C++ 참조 결과, 허용치 기준선이 main에 있다.
  - S1·S2·S3의 측정 결과와 결정 기록(래스터 방식, 이벤트 루프, 펜 백엔드, 형식 구조, 허용치)이 문서로 남는다.

### 1단계 — 우글거림 수학과 입력 (`ugu-base`, `ugu-motion`)

- **범위**: 결정적 수학, 노이즈, 모션 3종, 끊어진 선, 획 준비(리샘플·변위, 증분 기하), 1€ 필터, 획 샘플링 규칙, 브러시·지우개 프리셋 값.
- **완료 조건**
  - 장면 행렬 전 획 × 전 프레임의 준비된 점이 C++ `geometry`와 허용 오차 안에서 일치(M층).
  - 기록한 펜 입력의 1€ 필터 출력이 C++와 일치(I층).
  - Windows·macOS·Linux·wasm에서 Rust 결과가 비트 동일.
  - MotionTime, ClassicMotion, BrokenLine, StrokeStabilizer, Wobble 수학 테스트를 명세로 다시 씀.

### 2단계 — 래스터라이저와 렌더 엔진 (`ugu-raster`, `ugu-render`)

- **범위**: S1에서 정한 래스터라이저, 8비트 합성 정수식, 클립, 연산 재생(섹션 경계, reframe, 픽셀 선택 변형, 이미지), 레이어 계층 합성(그룹, 클리핑, 블렌드 4종, 불투명도), 타일 렌더와 증분 갱신, 정적 레이어 캐시(Drop 가드), 래스터 자산 캐시, 레이어 분할 프레임, 썸네일, 표시 배율 미리보기.
- **완료 조건**
  - 장면 행렬 전부가 F층 허용치 안. 넘는 장면은 사용자가 비교 뷰어로 승인(H층).
  - **사용자 승인**: 브러시 프리셋 17종과 모션 3종을 나란히 재생해 "그리는 느낌·우글거리는 느낌이 같다"는 확인.
  - 내부 계약(증분 = 전체, 영역 = 전체의 일부, 정적 캐시 = 비캐시) 통과, 네 플랫폼 골든 일치.
  - 같은 기기에서 전 프레임 렌더 시간과 최대 메모리를 C++와 비교한 측정 기록. 회귀 허용 범위는 0단계에서 정한다.
  - 이 시점의 Rust 결과를 동결 골든으로 커밋한다. 이후 C++ 참조는 갱신하지 않는다.

### 3단계 — 모델과 새 파일 형식 (`ugu-model`, `ugu-format`) — 2단계와 병렬 가능

- **범위**: 재설계한 모델(§3.1), 메모리 기준 한도, 불변식 검증, 형식 명세 문서, 컨테이너 읽기·쓰기, 버전·마이그레이션 골격, 클립보드 앱 전용 형식, 새 프리셋 형식.
- **완료 조건**
  - `docs/`에 형식 명세(구조, 필드, 한도, 버전 정책, 하위·상위 호환 규칙).
  - 형식 1판 골든 fixture가 저장소에 있고, 읽기 → 쓰기 → 읽기가 모델 동일, 저장 → 열기 → 렌더가 비트 동일.
  - 잘못된 파일(잘린 ZIP, 잘못된 헤더, 한도 초과, 압축 폭탄)을 모두 명확한 오류로 거절.
  - 형식·클립보드·프리셋 퍼저가 정해진 시간 동안 크래시 없음.

### 4단계 — 편집과 히스토리 (`ugu-edit`)

- **범위**: 순수 편집 함수와 단일 커밋 지점(D07), delta 히스토리(`Arc` COW, 메모리 회계, 개수·바이트 한도), 매크로, 병합, 선택 연산, 레이어 명령, 이미지 가져오기, 크기 변경·자르기, 텍스트 → 획(Rust 폰트 스택), bucket fill.
- **완료 조건**
  - 이식 대장에서 "유지할 행동"으로 분류한 DocumentHistory, LayerCommand, StrokeCommand, DocumentResize, DocumentLifecycle, SelectionClipboard, MaskRegression 항목이 Rust 테스트로 통과.
  - 무작위 편집 시퀀스 속성 테스트: 임의의 편집 → undo 전부 → 원래 문서와 동일, redo 전부 → 최종 문서와 동일.
  - 클립이 위에 있는 병합처럼 이미 알려진 결함(B-01 등)은 처음부터 고친 동작으로 테스트.

### 5단계 — 세션 계층과 내보내기 (`ugu-session`, `ugu-export`)

- **범위**: 상호작용 상태 기계(bool 약 20개 → `enum Interaction`, Q-04), 도구별 모듈, 뷰포트 수학, 펜 샘플 처리 규칙, 미리보기 스케줄러(warmup, 상호작용 프레임, 도구 참조, 선택 가시성, 더티 추적), 재생 타이밍, 문서 세션(열기·저장·자동저장·복구·종료를 하나의 상태 기계로, A-04), 원자적 쓰기, 단일 인스턴스, GIF·WebP·PNG/JPEG 내보내기.
- **완료 조건**
  - Ui*Tests에서 뽑은 상호작용 시나리오(입력 상태 기계, 펜·터치 우선순위, 제스처, 선택·변형 세션, pen-up 승격, 동기 렌더 0회 조건)가 창 없이 세션 테스트로 통과.
  - 저장 실패 주입 테스트(T-01): 읽기 전용·없는 폴더·디렉터리 경로·쓰는 중 오류에서 "수정됨 유지, 복구본 유지, 원본 보존, 안내".
  - 내보낸 GIF·WebP를 디코드한 프레임이 렌더 결과와 일치(GIF는 팔레트 양자화 규칙 안에서).
  - 입력 → 패치 준비까지의 헤드리스 지연 측정 기록.

### 6단계 — 웹 전환 (`ugu-wasm` + Svelte 셸 수정)

- **범위**: wasm-bindgen 바인딩, 워커 메시지 프로토콜 재정의(버전 필드 포함, W-08), 셸의 파일 열기·저장·자동복구를 새 형식으로, 디코드 예산(W-03/R11)을 처음부터 반영, 브라우저 시나리오 수정, Qt for WebAssembly·emsdk·`src/wasm` 제거.
- **완료 조건**
  - 수정한 브라우저 시나리오 전부 통과(장애 주입 시나리오 포함), itch.io 패키지 검사 통과.
  - 지금 웹판과 같은 기기에서 첫 로드 크기·시간, 그리기 반응, 메모리를 비교한 측정 기록.
  - 웹 빌드에 Qt 의존 0.
- **의미**: Rust 코어가 데스크톱보다 먼저 실제 사용자 환경에서 돈다.

### 7단계 — 데스크톱 셸 (`ugu-gpu`, `ugu-ui`, `ugu-platform`, `ugurugu`)

- **범위**: wgpu 캔버스, egui 패널·도크·대화상자 전부, 테마·강조 색상, 아이콘(지금도 벡터 폴리라인), Pretendard, i18n(ko/en/ja, 번역 완성도 게이트), 단축키 편집, 펜·제스처 백엔드, 클립보드, 파일 대화상자, macOS 파일 열기·창 크롬·메뉴, 업데이트(Velopack, Sparkle), 패키징(Windows Velopack, macOS 서명·공증 DMG, Linux AppImage).
- **완료 조건**
  - 기능 대조표(README 기능 + 메뉴·액션 58개 + 대화상자 + 도크)에서 모든 항목이 "동일" 또는 사용자가 승인한 "변경·제외".
  - GPU 표시 대 CPU 표시 비교 테스트(채널 ≤ 8) 통과.
  - 실제 펜 확인: Windows(Wacom, WinTab·Windows Ink), macOS. 압력, 지우개 끝, 근접 이탈, 측면 버튼.
  - **사용자 승인**: 실제 앱에서 그리는 느낌(지연, 필압 응답, 손떨림 보정)이 C++ 앱과 같다는 확인. 입력 → 화면 지연은 같은 기기에서 C++ 앱보다 나쁘지 않다는 측정.
  - 한국어·일본어 IME로 텍스트 도구와 레이어 이름 입력 확인.
  - 패키지 smoke를 Rust 앱용으로 다시 작성해 세 OS에서 통과.

### 8단계 — 베타, 출시, Qt 제거

- **범위**: 베타 채널(새 앱 정체성), 피드백 반영, 정식 출시, 그 뒤 C++ 소스·CMake·Qt CI 잡·`release.yml`의 C++ 경로 삭제, `THIRD_PARTY_NOTICES` 갱신(cargo-about 등), 문서 갱신.
- **완료 조건**
  - 베타 기간 동안 데이터 손실 버그 0건, 크래시율 기준 충족(기준은 이 단계 시작 때 정함).
  - 서명·공증·업데이트가 베타 채널에서 실제로 동작.
  - 저장소와 빌드에 Qt 의존 0, C++ 마지막 상태에 태그.

---

## 8. 결정 항목

### 확정 (2026-10-05)

| 항목 | 결정 |
|---|---|
| C++ `.ugu` 지원 | 하지 않는다. 레거시를 털어낸다 |
| 느낌 | 그리는 느낌과 우글거리는 느낌은 크게 변하지 않아야 한다 |
| 웹 | Qt를 걷어내고 Rust wasm 엔진으로 |
| macOS 업데이터 | Sparkle 유지 |
| 데스크톱 배포 정체성 | 새 앱 정체성(앱 id·설치 위치·확장자·업데이트 피드). C++ 2.x와 함께 설치할 수 있다 |
| WiggleWiggleTool `.wawa` 가져오기 | 넣지 않는다 |

### 남은 결정과 권장값

| 항목 | 선택지 | 권장 |
|---|---|---|
| 새 앱 이름과 확장자 | — | 사용자가 정함 |
| 웹 전환 시 기존 초안 | 안내 후 교체 / 이전 판을 별도 주소로 한동안 유지 | 안내 후 교체, 전환 전 버전에서 내보내기 안내 |
| 래스터라이저 | 자체 해석적 타일 래스터 / tiny-skia | 자체, 0단계 S1로 확정 |
| 웹 UI | Svelte 유지 / egui로 통일 | Svelte 유지, 데스크톱 출시 뒤 다시 판단 |
| 이벤트 루프 | 직접 winit 루프 / eframe | 직접 루프, S2로 확정 |
| 설정 이전 | 하지 않음 / 단축키·프리셋만 가져오기 | 하지 않음 |
| Linux 배포 | AppImage / Flatpak / 둘 다 | AppImage(Velopack)로 시작, 펜은 best effort부터 |
| 도크 | 창 안 도킹만 / OS 플로팅 창까지 | 창 안 도킹부터 |

---

## 9. 주요 위험과 대응

| 위험 | 영향 | 대응 |
|---|---|---|
| 새 래스터라이저의 느낌 차이 | 제품의 핵심 가치 훼손 | 0단계 허용치 기준선, S1 비교, 2단계 사용자 승인 게이트, 기하는 그대로 이식 |
| 에어브러시 누적 느낌 변화 | 부드러운 브러시의 질감 변화 | 8비트 정수 합성 유지, 누적 곡선 지표 |
| 펜 필압·지연 | 그리기 앱의 핵심 사용성 | S2 측정, 자체 펜 추상, 실제 펜 확인과 사용자 승인을 완료 조건에 포함 |
| egui 매 프레임 레이아웃 비용 | 그리는 중 프레임 저하 | S2 측정, 캔버스는 callback으로 분리 |
| IME | 텍스트 도구·이름 입력 불가 | S2에서 확인, 문제 시 플랫폼별 보완 |
| 형식 비호환에 대한 사용자 혼란 | 기존 파일이 열리지 않음 | 새 앱 정체성·새 확장자, C++ 2.x 병행 설치, 출시 안내 |
| 새 형식의 초기 설계 결함 | 첫 판부터 호환 부담 | 1판 전에 명세 검토, 버전·마이그레이션 골격과 골든을 1판부터 |
| 이중 유지 기간 | 피로, C++ 수정의 낭비 | C++ 웹 엔진 즉시 동결, 데스크톱은 세션 단계부터 기능 동결 |
| macOS 통합(파일 열기, 메뉴, 창 크롬, Sparkle) | 네이티브 사용감, 업데이트 실패 | objc2 소규모 모듈, 베타에서 실제 확인 |

---

## 부록 A. 기존 발견과 새 구조의 대응

| 기존 발견 | 새 구조에서 |
|---|---|
| A-01 document ↔ io ↔ render 순환 | 크레이트 의존 표로 불가능. 바이트 계획 자체가 없어진다 |
| A-02 / Q-02 / D07 컨트롤러 책임 과다, 실패 배관, no-op 계약 | 순수 편집 함수 + 단일 커밋(`Committed / Unchanged / Rejected`) |
| A-03 / Q-04 CanvasWidget god class, 상호작용 bool | 세션 계층의 `enum Interaction` + 도구별 모듈 + 미리보기 스케줄러 분리 |
| A-04 MainWindow 안의 세션 상태 기계 | 세션 계층의 문서 세션 상태 기계, 명령 레지스트리 |
| A-05 암묵적 포맷 호환 정책 | 새 형식의 명세, 버전·마이그레이션 함수, 버전별 골든 |
| A-06 Stroke 표현 | `Op` enum, 레이어 우글거림 override는 한 쌍 |
| Q-01 재생 루프 두 벌 | 하나의 재생 루프. 표시 배율은 전략으로 주입 |
| B-01 클립 위 병합 | 처음부터 고친 동작으로 테스트 |
| B-02 캐시 대기 영구화 | Drop 가드 |
| B-12 / B-13 저장·복구 실패 경로 | 문서 세션 상태 기계 + 실패 주입 테스트 |
| T-04 합성 펜 시나리오 | 펜 처리 규칙을 순수 로직으로 옮겨 합성 샘플로 테스트 |
| T-06 포맷 fixture 빈약 | 새 형식은 버전마다 골든 |
| T-07 OS별 골든 | 결정적 수학으로 모든 OS·wasm 단일 골든 |
| R-01 이미지 포맷 플러그인 누락 | 디코더를 앱에 정적 포함 |
| U-01 Qt 표준 문자열 미번역 | 모든 문구를 앱 번역 파일이 소유 |
| `.wwpreset` 값 타입 손실(의심) | 타입 있는 설정·프리셋 형식으로 구조적으로 사라짐 |
