# Ugurugu 데스크톱 앱 종합 분석 보고서

- 기준: `main` `60a11be` (작업 트리 clean), 2026-10-03
- 범위: 데스크톱 앱(Windows / macOS)만 다룬다. 요청에 따라 그 밖의 포팅은 제외했다.
- 방법: 6개 영역(문서 모델·히스토리 / 렌더 엔진 / IO·앱 서비스 / 캔버스·입력·표시 / 메인 윈도우·UX / 빌드·배포·라이선스·테스트)을 Sonnet 서브 에이전트가 정적으로 분석했다. 나는 Windows 클린 빌드, 전체 테스트, 설치 트리 생성을 직접 실행했고, 결과를 취합했다. 높음 등급과 주요 중간 등급 지적은 코드나 산출물로 직접 다시 확인했다.
- 분석 단계에서는 소스 코드를 수정하지 않았다. 이후 B-01·B-02 수정은 PR #10에서 이 보고서와 함께 반영했고, 진행 상태는 `docs/project-status.md`에서 관리한다.

---

## 요약

Ugurugu는 단독 개발 프로젝트로서는 기반이 매우 단단하다. 트랜잭션형 편집과 delta 히스토리, 원자적 저장, 출력 상한을 둔 압축 해제, 결정적 노이즈 기반 우글거림을 갖췄고, 의존성은 해시로 고정했으며 macOS 서명·공증 파이프라인과 엄격한 CI(경고=오류, ASan/UBSan, 커버리지 70%, 번역 게이트)가 있다. Windows 클린 빌드는 소스 경고 0으로 성공했고, 테스트 13개 스위트(QTest 674건)는 모두 통과했다. 치명 등급은 없다. 남은 위험은 **실패·경계 경로, 배포본 실검증, 거대 클래스 세 개**(CanvasWidget, DocumentController, MainWindow)에 몰려 있다.

**가장 시급한 문제 3가지**

1. **결과물을 조용히 바꾸는 정확성 결함.** 바로 위 형제가 클리핑 레이어일 때 '아래 레이어와 병합'을 하면 픽셀이 바뀐다(B-01). 그리는 중 합성 프레임이 최종 렌더와 최대 ±2 다른데 그 프레임이 정확한 프레임으로 캐시된다(B-04). 둘 다 "미리보기 = 최종" 계약과 정면으로 충돌한다.
2. **메모리 압박이나 고배율에서 앱이 멈추거나 폭주하는 경로.** 렌더 캐시의 대기 표시가 예외에 안전하지 않아, 한 번 `bad_alloc`이 나면 같은 레이어나 자산을 기다리는 모든 렌더가 영구 대기한다(B-02). 확대한 상태에서 캔버스 가장자리가 보이면 그림자 pixmap을 캔버스 전체 크기(400%·DPR 1.5에서 약 457 MB)로 만든다(B-03).
3. **배포본이 개발 환경과 다르게 동작하는데, 이를 잡을 검증이 없다.** 설치 트리를 직접 만들어 확인했다. 이미지 삽입이 WebP/TIFF를 광고하지만 플러그인이 동봉되지 않는다(macOS는 GIF도 빠짐, R-01). 한국어·일본어 UI에서 Qt 표준 문자열(OK/Cancel, 색 대화상자 등)이 번역되지 않는다(U-01). CI는 offscreen 테스트만 돌리고 서명된 macOS 앱을 한 번도 실행하지 않는다(R-04, R-05).

---

## 목차

0. [읽는 법과 분석의 한계](#0-읽는-법과-분석의-한계)
1. [빌드·테스트 실행 결과](#1-빌드테스트-실행-결과)
2. [아키텍처](#2-아키텍처)
3. [코드 품질](#3-코드-품질)
4. [버그 및 잠재적 문제](#4-버그-및-잠재적-문제)
5. [성능](#5-성능)
6. [UI/UX](#6-uiux)
7. [빌드·배포](#7-빌드배포)
8. [라이선스·저장소 관리](#8-라이선스저장소-관리)
9. [테스트](#9-테스트)
10. [잘 된 부분 — 유지할 설계](#10-잘-된-부분--유지할-설계)
11. [우선순위별 개선 로드맵](#11-우선순위별-개선-로드맵)
12. [부록](#부록)

---

## 0. 읽는 법과 분석의 한계

**심각도**

| 등급 | 기준 |
|---|---|
| 치명 | 흔한 사용 경로에서 데이터 손실·크래시·보안 침해. **이번 분석에서는 0건** |
| 높음 | 결과물이 조용히 틀리거나, 앱이 멈추거나, 대량 메모리를 할당하는 경로. 코드로 확인됨 |
| 중간 | 사용자 체감 결함, 드문 조건의 안정성 문제, 검증 공백, 구조적 위험 |
| 낮음 | 품질·일관성·잔손질. 영향이 작거나 드묾 |

**근거 수준**

- **직접 확인**: 최종 작성자인 내가 코드나 실행 결과로 다시 확인한 항목이다.
- **확인됨**: 서브 에이전트가 해당 줄을 읽고 확인했다. 줄 번호는 `60a11be` 기준이다.
- **추측**: 코드 경로는 있지만 재현이나 측정을 하지 않았거나, Qt·OS·서드파티의 동작에 기대는 주장이다. 본문에 명시했다.

**ID 체계**: `A`(아키텍처), `Q`(품질), `B`(버그), `P`(성능), `U`(UI/UX), `R`(빌드·배포), `L`(라이선스·저장소), `T`(테스트). `docs/project-status.md`에 이미 있는 ID(D01–D25, R06–R14)와 관련되면 **관련 ID**에 적었다. 그 문서에 없는 새 내용을 우선 서술했다.

**한계**

- macOS 빌드는 하지 않았다. Metal 경로, Sparkle, `MacWindowChrome.mm`은 정적 분석만 했다.
- 네이티브 플랫폼(`QT_QPA_PLATFORM=windows`) UI 테스트는 실행하지 않았다. 사용자 데스크톱에 창이 뜨고 포커스를 빼앗기 때문이다. 그래서 GPU 표시 테스트 2개는 offscreen에서 건너뛴 상태 그대로다.
- 실제 펜, 퍼저, 커버리지, 벤치마크는 실행하지 않았다. **성능 항목의 ms·MB 값은 모두 측정값이 아니라 코드에서 계산한 추정치**다. 고치기 전에 기존 방식(같은 조건 A/B)으로 측정해야 한다.
- 설치 트리는 로컬 Qt 6.11.2로 만들었다. 배포는 Qt 6.11.1이지만 같은 deploy 스크립트를 쓰므로 구성은 같을 것으로 본다(추측).

---

## 1. 빌드·테스트 실행 결과

### 1.1 환경

Windows 11 Pro x64 / VS 18 BuildTools, ClangCL 툴셋(MSBuild 18.10.1) / Qt 6.11.2 msvc2022_64 / CMake 4.4.3. 프리셋은 `windows-release`를 썼고, 별도 빌드 디렉터리 `out/build/analysis`에 클린 구성했다.

```bash
cmake --preset windows-release -B out/build/analysis -DCMAKE_PREFIX_PATH=C:/Qt/6.11.2/msvc2022_64
```

```bash
cmake --build out/build/analysis --config Release --parallel
```

```bash
ctest --test-dir out/build/analysis -C Release -j4 --output-on-failure
```

### 1.2 결과

| 단계 | 결과 | 비고 |
|---|---|---|
| 구성 | 성공(26 s), 경고 2종 | ① `Qt6::GuiPrivate` 사용 경고(`cmake/UguruguDependencies.cmake:10`, `UguruguTargets.cmake:39,133`). QRhi와 WinTab 때문에 의도된 것이다. ② zlib에 대한 AUTOGEN author 경고. `UguruguCompression.cmake:10-18`이 `CMAKE_AUTOMOC=ON` 상태에서 zlib를 가져오기 때문이다. libwebp는 같은 경우 AUTOMOC를 잠시 끈다(`UguruguDependencies.cmake:69-81`) |
| 빌드 | **성공. 소스 컴파일 경고 0** (`/W4 /permissive-`) | 링커 경고 1건: `lld-link : warning : found both wWinMain and WinMain; using latter`. `Qt6EntryPoint.lib`가 두 심볼을 모두 정의한다(`llvm-nm`으로 직접 확인). 무해하다 |
| 산출물 | `Ugurugu.exe`, `ugurugu_tests.exe`, `ugurugu_package_smoke.exe` | 보조 도구(벤치마크, probe)는 `EXCLUDE_FROM_ALL`이라 빌드되지 않는다 |
| CTest | **13/13 통과**, 15.1 s | 모두 `QT_QPA_PLATFORM=offscreen`이다 |
| QTest 집계 | **674건 통과, 0 실패, 2 건너뜀** | 35개 테스트 클래스의 `initTestCase`/`cleanupTestCase` 70건을 포함한다. 건너뛴 것은 GPU 표시가 있어야 도는 `gpuDisplayMatchesTheSoftwareDisplay`, `drawsWithoutRepaintingTheCanvasWhileOtherWidgetsRepaint` 두 개다 |
| 설치 트리 | `cmake --install`로 스크래치 폴더에 생성, 70 MB | 아래 1.3 |

스위트별 결과(통과/건너뜀): app 10, document 204, render 171, gif 18, webp 5, mask 13, release_notes 7, stabilizer 8, ui_shell 67, ui_selection 58, ui_viewport 59/2, ui_drawing_tools 30, ui_session 24.

### 1.3 설치 트리에서 직접 확인한 사실

- `translations/`에는 병합본 `qt_ko.qm`, `qt_ja.qm`만 있고 `qtbase_*.qm`은 없다. 앱은 `qtbase` 접두로 로드한다. → U-01
- `imageformats/`에는 `qgif`, `qico`, `qjpeg`, `qsvg`만 있고 `qwebp`, `qtiff`는 없다. → R-01
- 고지 문서에 없는 바이너리가 동봉된다. `opengl32sw.dll`(Mesa llvmpipe, 20.6 MB), `D3Dcompiler_47.dll`(4.2 MB), `Qt6Network.dll`과 `tls/`, `networkinformation/` 플러그인이다. → L-01, R 낮음표
- 라이선스 파일 `LICENSE`, `LGPL-3.0.txt`, `THIRD_PARTY_NOTICES.md`, `Pretendard-OFL.txt`, spdlog, libwebp(+PATENTS), zlib, Velopack은 모두 설치된다.

---

## 2. 아키텍처

### 2.1 구조 개요

**규모** (`src/wasm` 제외 `src` 58,013줄, `tests` 약 31.2K줄)

| 모듈 | 파일 | 줄 | 책임 |
|---|---|---|---|
| `src/document` (+`history/`) | 34 | 9,867 | 문서 모델(Document/Layer/Stroke), 변경 관문 `DocumentController`, delta 히스토리, 선택 연산·마스크 |
| `src/render` (+`engine/`) | 45 | 9,712 | 우글거림 모션, 획 래스터, 레이어 계층 합성, 증분 렌더, 정적 레이어·래스터 캐시 |
| `src/io` (+`serializer/`) | 31 | 9,159 | `.ugu` JSON 코덱·검증·자산표·bounded inflate, GIF/WebP, 구형 Wawa import, 클립보드 |
| `src/app` | 21 | 1,902 | 복구 저장소·작성기, 인스턴스 락, 로깅, 업데이트(Velopack/Sparkle), 메모리 예산 |
| `src/brush`, `src/input` | 6 | 595 | 브러시 프리셋, 1€ 필터 안정화 |
| `src/ui` | 123 | 26,448 | MainWindow, CanvasWidget(7,100줄), 도크, 팝오버, 대화상자, QRhi 표시 창 |
| `src/main.cpp` | 1 | 330 | 시작 시퀀스 |

**빌드 타깃** (`cmake/UguruguTargets.cmake`): `ugurugu_core`(STATIC. 엔진과 데스크톱 서비스. Qt Core/Gui, spdlog, zlib, libwebp만 쓰고 **Widgets는 링크하지 않음**, `:9-25`) → `ugurugu_ui`(STATIC. Widgets, Concurrent, GuiPrivate, 셰이더 4개) → `Ugurugu`(실행 파일과 플랫폼별 UpdateController) / `ugurugu_tests`(단일 exe, 13개 CTest 항목). 의존성은 CMake 3.31+, C++23, Qt ≥ 6.10(배포 6.11.1 정확 고정)이고, FetchContent 5종(zlib 1.3.2, spdlog 1.16.0, libwebp 1.6.0, Sparkle 2.9.6, Velopack 1.2.0)은 모두 URL과 SHA-256으로 고정돼 있다.

**계층과 데이터 흐름**

```
 main.cpp ─▶ MainWindow (액션·메뉴·도크·저장/복구/자동저장/내보내기 조율)
               │
               ├─ CanvasWidget ── 입력(마우스/태블릿/터치) → StrokeStabilizer
               │      │             → IncrementalStrokeRenderer(타일 패치)
               │      │             → LayerSplitFrame 합성 → CanvasDisplayWindow(QRhi) 또는 software paint
               │      └─ 워커 풀: 프레임 warmup(≤8), 인터랙션 프레임(1), 도구 참조(1)
               │
               ▼
        DocumentController ── 후보 Document → prepare(검증·크기 계획, io/) → DocumentDelta
               │                → DocumentUndoStack(개수 64 + 192 MiB) → documentChanged
               ▼
        RenderEngine(정적 facade) ── LayerCompositionPlan → renderLayerHierarchy
                                      → (StaticLayerCache) → LayerOperationReplay / DisplayScaleReplay
                                      → StrokeRenderer(prepare: 리샘플·StrokeMotionModel 변위 → paint)
```

- **우글거림 모델**: 변위는 `f(seed, frame|pose, arcLength, sampleIndex, channel)` 형태의 순수 함수다. splitmix64 해시 value noise와 smoothstep을 쓴다(`src/render/DeterministicNoise.cpp:15-60`). 시간 모델은 Classic(v1 호환, 프레임마다 독립), Smooth(키 포즈 보간), Stepped(포즈 유지) 세 가지다(`MotionTimeModel.cpp:43-71`). 프레임 사이에 공유 상태가 없어서 프레임 단위 병렬 렌더가 가능하다.
- **스레딩**: 모든 가변 상태는 GUI 스레드가 소유한다. 워커에는 `shared_ptr<const Document>`, `PreparedDocument`(불변 스냅샷), `shared_ptr<atomic_bool>` 취소 토큰만 넘긴다. 결과는 큐드 `QFutureWatcher`와 generation 카운터로 받는다. 프로세스 전역 가변 상태는 mutex로 보호되는 두 캐시(`StaticLayerCache` 256 MiB, `RasterAssetCache` 128 MiB)뿐이다.
- **저장**: `serializationSnapshot()` 결과를 1스레드 풀에서 `QSaveFile`로 쓴다. 완료 시 `contentRevision`이 같을 때만 저장됨으로 표시한다. 자동저장은 30초마다, 그리고 창이 비활성이 될 때 `RecoveryWriter` 전용 스레드로 쓴다.

### 2.2 평가

**좋은 점**: 엔진의 UI 독립성은 실제로 지켜진다. `src/document`, `render`, `io`, `brush`, `input`에는 `ui/`나 QtWidgets include가 0건이다. 변경은 `DocumentController` 한 곳으로만 들어오고, 워커와의 공유는 불변 스냅샷으로 통일돼 있다.

**문제**: 계층 사이에 순환 의존이 있고, 책임이 세 클래스(UI 2개, 코어 1개)에 몰려 있다. 아래 이슈들이다.

#### A-01 · 중간 · document ↔ io ↔ render 순환 의존
- **위치**: document→io `src/document/DocumentController.hpp:9`, `DocumentController.cpp:16`, `DocumentControllerLayers.cpp:10` / document→render `DocumentController.cpp:17-18`, `DocumentControllerLayers.cpp:11`, `SelectionOperation.cpp:8`, `SelectionVisibility.cpp:6` / render→io `src/render/RasterAssetCache.cpp:7` / io 내부 순환 `src/io/serializer/DocumentJsonCodec.cpp:10` ↔ `src/io/DocumentSerializer.cpp:10` / 역방향 app→ui `src/app/UpdateControllerWindows.cpp:7`(`ui/SettingsDialog.hpp`)
- **문제**: 도메인 불변식(`prepare`, `validateDocument`, `normalizeAndValidate`)을 `io/`가 소유한다. 그래서 컨트롤러는 io에 의존하고, io는 document 헤더 36곳을 include한다. 선택 가시성 계산은 렌더 엔진을 부른다.
- **영향**: 모듈을 따로 테스트하거나 교체할 수 없다. 변경 하나가 재컴파일과 회귀 범위를 넓힌다. 데스크톱 빌드에서는 `ugurugu_core`가 엔진과 서비스를 한 라이브러리로 묶어서(`UguruguTargets.cmake:9`) 경계 위반을 컴파일 타임에 잡지 못한다.
- **개선**: ① `PreparedDocument`와 검증 함수를 `document/`로 옮겨 io가 그쪽에 의존하게 한다. ② `selectionHasVisibleLayerPixels`와 그 캐시를 `SelectionService`(render 또는 ui 쪽)로 뺀다. ③ 데스크톱에서도 `ugurugu_engine`(엔진 소스 목록만)을 별도 STATIC 라이브러리로 만들어 경계를 강제한다. ④ `MemoryBudget.hpp`를 `app/`에서 엔진 쪽으로 옮긴다.
- **근거**: 확인됨 · **관련 ID**: D22

#### A-02 · 중간 · `DocumentController` 책임 과다 (cpp 3개 3,840줄, 헤더 430줄)
- **위치**: `src/document/DocumentController.hpp:34-89`(UndoStack 선언과 friend), `:350-428`, 히스토리 배관 `DocumentController.cpp:105-380`, `:1190-1680`(약 700줄), 16개 위치 인자 생성자 `:118-133`
- **문제**: 변경자 약 45개, 직렬화 진입점, 선택 가시성 캐시, 히스토리 트랜잭션, 이펙트 발행, 문서 교체가 한 QObject에 있다. 실패 전달은 호출자가 `failHistoryMacro()`/`rejectHistoryMutation()`을 약 120곳에서 직접 불러야 한다(Q-02). `DocumentUndoStack`은 컨트롤러 private 멤버(`m_undoStack.m_moving`)를 10곳에서 직접 건드리고, UI 테스트 클래스(`MainWindowTestAccess`)까지 friend로 둔다(`:22,:427`).
- **영향**: 변경자 하나를 단위 테스트하려 해도 QObject, 직렬화기, 히스토리가 모두 필요하다. 실패 경로를 하나만 빠뜨려도 부분 커밋 회귀가 생긴다. `quint64` 4개와 `qint64` 2개를 위치로만 구분하는 인자는 순서를 바꿔 써도 컴파일러가 잡지 못한다(`.clang-tidy`가 `bugprone-easily-swappable-parameters`를 끈다).
- **개선**: (1) 변경자를 순수 함수 `std::optional<Edit> editSetLayerOpacity(const Document&, QUuid, qreal)`로 바꾸고, 컨트롤러는 `return commit(editXxx(...))` 한 줄로 만든다. `commit`이 `committed / unchanged / rejected(reason)`을 반환한다(D07 해결 경로). (2) `HistoryTransactionManager`를 추출한다. (3) `struct CommitSnapshot { quint64 node; quint64 revision; qint64 compactSize; }`로 인자를 묶는다. 기존 테스트(DocumentHistory 1,573줄, LayerCommand 1,173줄)가 이 리팩터링을 보호한다.
- **근거**: 확인됨 · **관련 ID**: D07, D22

#### A-03 · 중간 · `CanvasWidget`이 god class다 (헤더 723줄, cpp 6개 약 7,100줄, 데이터 멤버 약 177개, 상호작용 bool 13개)
- **위치**: `src/ui/CanvasWidget.hpp:45-721`. `usingGpuDisplay()` 분기가 `CanvasWidget.cpp:823,841,852,863,873,883`, `CanvasWidgetEvents.cpp:240`, `CanvasWidgetPreview.cpp:404`, `CanvasWidgetTools.cpp:156,364`에 흩어져 있다. `CanvasDisplayWindow`가 friend로 내부를 직접 읽는다(`CanvasDisplayWindow.cpp:321,385,433-434`).
- **문제**: 입력 라우팅, 획 입력, 뷰포트(줌·팬·회전·제스처), 선택·변환 세션, 텍스트 배치, 프리뷰 스케줄링(워커 3종과 캐시), 표시 백엔드라는 7개 책임이 한 클래스에 있다. 분할 프레임과 래스터 프레임 두 갈래를 위젯이 직접 알기 때문에 같은 분기가 `activeStrokePreview`(`CanvasWidgetPreview.cpp:244-298` 대 `308-364`)와 `resolveDisplayedFrame`(`578-600` 대 `603-635`)에 중복된다.
- **영향**: 상태 머신 버그(Q-07, B-21)와 중복의 근본 원인이다. 렌더 타입이 헤더 멤버라서 렌더 쪽을 바꾸면 UI 전체와 테스트가 다시 컴파일된다.
- **개선**: 위험이 낮은 순서로 뽑아낸다. ① `ToolReferenceRenderer`(멤버 9개, `CanvasWidgetTools.cpp:735-939`, 이미 자족적이다) ② `TextPlacementTool` ③ `ViewportState`(순수 함수 모음인 `CanvasViewport.hpp`를 확장해 위젯 없이 테스트) ④ `enum class Interaction`으로 bool 13개 통합(Q-07) ⑤ `DisplaySource` 인터페이스로 friend 접근 제거 ⑥ `PreviewPipeline`(FrameCacheWarmup / InteractionFrameWorker / ActiveStrokePreview). RenderEngine 쪽에 `prepare(frame, layer) → composeRegion(patch)` 편집 세션 facade를 두면 이중 분기 약 150줄이 없어진다. ⑦ `SelectionController`, `StrokeInputTool`. 각 단계는 기존 UI 테스트(Viewport 2,847줄, Selection 3,945줄, DrawingTool 1,776줄)가 보호한다.
- **근거**: 확인됨 · **관련 ID**: D22

#### A-04 · 중간 · `MainWindow`에 세션·저장·복구 상태 머신이 들어 있다 (4개 파일 3,813줄)
- **위치**: `src/ui/MainWindow.cpp`(1,707줄) 중 파일·세션 로직 `:281-1100`, `:1384-1591`. 복구 플래그가 `MainWindow.hpp:185-197`에 흩어져 있다. `createActions`는 881줄이다(`MainWindowActions.cpp:77-957`). 액션은 `findChild<QAction*>(QStringLiteral("..."))` 문자열 조회 40여 회로 찾는다(`:959-1078`, `MainWindowExport.cpp:238-253`, `main.cpp:247-252`).
- **문제**: 저장, 복구, 자동저장, 내보내기, 업데이트 연결이 UI 클래스 안에 있다. objectName 오타는 조용히 null이 된다.
- **영향**: 데이터 보존과 직결되는 로직(B-12, B-13)을 UI 없이 테스트할 수 없다.
- **개선**: `DocumentSession`(열기, 저장, 자동저장, 복구, 종료 정책. R06/D19의 pending edit 정책도 여기에 둔다)과 `ActionRegistry`(enum 키, 타입 있는 조회)를 분리한다. 도구 액션 7개는 테이블로 만든다.
- **근거**: 확인됨 · **관련 ID**: D22, R06, D19

#### A-05 · 중간 · `.ugu` 포맷의 호환 정책이 암묵적이다
- **위치**: `src/io/serializer/SerializerSchema.hpp:19-23`(schema 13 / algorithm 3), 읽기 범위 `src/io/DocumentSerializer.cpp:728-746`, 새 버전 거부 문구 `:733-735`, 쓰기 `DocumentJsonCodec.cpp:1538-1539`, 구버전 분기 `strokeFromJson` `:969-1446`(약 478줄, 4단 중첩 `fileSchemaVersion` 게이트), 레거시 확장자 제자리 저장 `src/ui/MainWindowActions.cpp:115-122` 대 주석 `MainWindow.cpp:106-111`
- **문제**: ① 더 새로운 파일은 "This project version is not supported."로만 거절한다. schema 번호와 업데이트 안내가 없다. ② `algorithmVersion`은 범위만 검사하고 버린다. 렌더 알고리즘을 바꾸면 기존 파일의 모양을 보존할 수단이 없다. ③ 명시적 마이그레이션 단계 없이, 읽기 코드 곳곳에 `fileSchemaVersion` 분기가 46줄 박혀 있다. ④ 열고 저장하면 백업 없이 최신 스키마로 덮어쓴다. 알 수 없는 필드는 사라진다. `.wagle`/`.wobble`도 Ctrl+S로 제자리에서 덮어쓴다. ⑤ 포맷 명세 문서가 없다.
- **영향**: 사용자는 파일 오류의 원인을 알기 어렵다. 스키마를 올릴 때마다 분기가 늘어 회귀 위험이 커진다.
- **개선**: 오류 문구에 "schema N, 이 빌드는 13까지 지원"을 넣는다. `migrateToCurrent(QJsonObject&, int from)` 단계 함수로 분리한다. `docs/`에 포맷과 호환 정책(올리는 기준, additive 허용 범위)을 쓴다. 레거시 확장자는 Save를 Save As로 승격한다. 덮어쓰기 업그레이드 시 `.bak` 1세대를 옵션으로 둔다.
- **근거**: 확인됨 · **관련 ID**: 없음(D22 인접)

#### A-06 · 낮음 · 데이터 모델 표현
- **위치**: `src/document/Document.hpp:175-198`(`Stroke`: mode 1개, optional 6개, `QImage` 2개), `:232-233`(레이어 우글거림 override가 optional 두 개), pose 클램프 중복 5곳(`DocumentController.cpp:934-938,946-953,1107-1112,1143-1157`, `DocumentControllerLayers.cpp:965-969`), 우글거림 쌍 병합 특수 함수 `DocumentDelta.cpp:119-170`
- **문제**: `Stroke`는 표현 가능한 상태가 유효한 상태보다 훨씬 많다. 유효 조합은 `DocumentOperations.cpp:118-193`, `DocumentValidation.cpp:301-330`, `StrokeMask.cpp:262-267`, `DocumentControllerStrokes.cpp:145-166` 네 곳에서 따로 검사한다. 우글거림 override는 항상 쌍으로 움직이는데 타입이 이를 표현하지 않는다.
- **영향**: StrokeMode를 추가할 때 고칠 곳이 4곳 이상이다. 일반 Paint 획도 모든 optional의 크기를 지고 다니므로 복사 비용(P-04)이 커진다.
- **개선**: 최소한 `Stroke::wellFormedFor(epoch)` 하나로 검사를 모은다. `struct WobbleSettings { qreal amount; MotionSettings motion; }`와 `std::optional<WobbleSettings> Layer::wobbleOverride`를 도입한다. 직렬화 키는 그대로 매핑하므로 포맷은 바뀌지 않는다. 중기적으로는 mode별 payload를 `std::variant`로 나눈다.
- **근거**: 확인됨 · **관련 ID**: 없음

---

## 3. 코드 품질

**총평**: 이름은 일관되고 서술적이며, "왜"를 적는 주석 습관이 좋다(예: `src/ui/CanvasWidget.cpp:159-164`의 PreciseTimer 근거, `Document.hpp:239-242`의 QUuid 주의). clang-format과 clang-tidy가 CI에서 강제되고, 모든 파일에 SPDX 헤더가 있다. 약점은 **초장문 함수, 계약에 영향을 주는 중복, 호출자 없는 코드, 수동 실패 배관**이다.

#### Q-01 · 중간 · native 재생과 display-scale 재생이 같은 순서 계약을 두 구현으로 가진다
- **위치**: `src/render/engine/LayerOperationReplay.cpp:468-636` 대 `src/render/engine/DisplayScaleReplay.cpp:242-448`(합성 경계, reframe, 섹션 평탄화 약 150줄 복사) / `applyPixelSelectionOperation` 세 벌: `LayerOperationReplay.cpp:332-406`, `StrokeCoverageRenderer.cpp:437-554`, `DisplayScaleReplay.cpp:71-163` / `maskPath` 두 벌: `LayerOperationReplay.cpp:29-58`, `IncrementalStrokeRenderer.cpp:19-48`
- **문제**: 연산 재생 순서는 미리보기와 최종 렌더가 같아야 하는 핵심 계약이다. 그런데 구현이 둘이라 한쪽만 고치면 둘이 갈라진다.
- **영향**: "미리보기 = 최종" 계약이 회귀할 수 있는 가장 큰 구조적 통로다.
- **개선**: 재생 루프를 하나로 합치고, native와 display-scale의 차이(표면 크기, 연산별 apply)는 전략 객체로 주입한다. 합친 결과가 비트 동일한지 `LegacyRenderGoldenTests`와 RenderPreview 테스트로 확인한다.
- **근거**: 확인됨 · **관련 ID**: D23("샘플러·epsilon 의미가 다른 코드는 모양만 보고 합치지 않음"을 지켜야 함)

#### Q-02 · 중간 · 실패 전달을 호출자 규율에 맡긴다
- **위치**: `failHistoryMacro()`/`rejectHistoryMutation()` 호출 `DocumentController.cpp` 40건, `DocumentControllerLayers.cpp` 29건, `DocumentControllerStrokes.cpp` 51건 / 누락: `applyMotionPreset`(`DocumentController.cpp:1099-1127`)과 `setActiveLayer`(`:856-869`)의 no-op 반환 / 같은 값 setter를 실패로 처리하는 곳: `setBackground` `:881-892` / `mergeId = 0`이 "병합 안 함"인데 `id() >= 0`이면 병합 후보로 보는 규칙(`DocumentUndoStack.cpp:136`)
- **문제**: 실패 경로를 하나만 빠뜨려도 매크로가 부분적으로 진행된다. 결과는 `void`나 `bool`로 뭉뚱그려진다. 예를 들어 리사이즈가 획 예산 초과로 실패해도 UI는 "더 작은 크기를 시도하라"고 안내한다(`DocumentController.cpp:747-766`, `MainWindow.cpp:1141-1144`).
- **영향**: 사용자는 실패 이유를 알 수 없고, 코드를 고칠 때마다 회귀 위험이 있다.
- **개선**: A-02의 `commit(optional<Edit>)` 단일 지점으로 모은다. `constexpr int noMergeId = -1`을 쓴다.
- **근거**: 확인됨 · **관련 ID**: D07

#### Q-03 · 중간 · 데스크톱 앱에서 도달하지 않는 코드가 크다
- **위치**:
  - 편집 API `moveStrokes/scaleStrokes/rotateStrokes/flipStrokes`(`DocumentControllerStrokes.cpp:275-362`), `duplicateStrokes`(`:405-637`), `transformStrokes`의 마스크 없는 분기(`:849-951`), `removeStrokes`(`:953-992`), `setImageTransform`(`DocumentControllerLayers.cpp:333-378`). `src/ui`와 `src/app`에서 호출하는 곳이 **0건이다(직접 확인)**.
  - `duplicateStrokes` 안의 도달 불가 분기: `:504-541`, `:556-559`, `:565-569`
  - `StrokePresence` 이펙트와 `strokePresenceChanged` 신호. 구독자가 없다(생성 `DocumentControllerStrokes.cpp:205-216`, 신호 `DocumentController.hpp:315-319`). 그런데도 선택 안에서 그릴 때마다 비용을 낸다(P-04).
  - 팝오버: `ToolPopover.*`, `PopoverToolButton.cpp:21-38,62-66,98-135`. `setPopover` 호출처가 없다. `*PopoverPanel`은 실제로는 ToolDock의 스택 페이지다(`ToolDock.cpp:61-71`).
  - `setAnimating`의 `previewSizeChanged` 분기는 항상 false다(`CanvasWidget.cpp:1409-1415,1432`).
- **문제**: 테스트만 이 코드를 붙들고 있다. D01과 D20이 가리키는 마스크 없는 변환 경로도 데스크톱 프로덕션에서는 쓰이지 않는다. D01 중 떠 있는 선택 세션 부분은 프로덕션 경로이니 구분해야 한다.
- **영향**: 약 400줄, 이펙트 2종, 신호 2종, UI 핸들러, 다수 테스트가 쓰이지 않는 동작을 유지한다. 유지보수 우선순위가 왜곡된다.
- **개선**: 먼저 도달 불가 분기와 `StrokePresence`를 지운다. 나머지 API는 엔진의 다른 빌드 타깃에서 쓰는지 확인한 뒤 삭제하거나 `tests/support` 어댑터로 옮긴다. D01/D20의 우선순위를 다시 매긴다. 팝오버는 이름을 `*SettingsPage`로 바꾸고 죽은 배선을 지운다.
- **근거**: 직접 확인(호출자 grep) · **관련 ID**: D01, D20, D23

#### Q-04 · 중간 · 캔버스 상호작용 상태가 bool 13개이고, 6곳의 조건 목록이 서로 다른 부분집합이다
- **위치**: 조건 목록 `CanvasWidget.cpp:1030-1031`(Escape), `CanvasWidgetEvents.cpp:190-192,202-204`(NativeGesture), `:714-715`(wheel), `CanvasWidgetTools.cpp:567-569`(터치 충돌), `CanvasWidgetPreview.cpp:1690-1692`(재생), `CanvasWidgetTools.cpp:998-1002` / 선언 `CanvasWidget.hpp:651-664`
- **문제**: 이미 누락이 있다. 트랙패드 핀치 가드에는 `m_panning`, `m_zoomDragging`, `m_rotatingCanvas`, `m_pickingColor`가 없다(팬 중 핀치로 줌이 바뀜). `advanceFrame`에는 `m_textDragging`이 없다. Escape 정리 분기에는 `m_tabletSequence`가 없다(B-21). 취소 호출 목록도 4곳에 복제돼 있다.
- **영향**: 도구나 제스처를 추가할 때마다 6곳을 맞춰야 한다.
- **개선**: `enum class Interaction { None, Stroke, Pan, ZoomDrag, Rotate, ColorPick, SelectionMove, AreaSelect, TextDrag }` 하나로 상태를 두고, `busy()`, `blocksGestures()`, `blocksPlayback()` 술어 한 곳에서 판정한다. 입력 원천(tablet/touch sequence)은 별도 라우터가 가진다.
- **근거**: 확인됨 · **관련 ID**: D03

#### Q-05 · 낮음 · 긴 함수 (상위 발췌, 대략 줄 수)

| 함수 | 위치 | 줄 |
|---|---|---|
| `MainWindow::createActions` | `src/ui/MainWindowActions.cpp:77-957` | 881 |
| `strokeFromJson` | `src/io/serializer/DocumentJsonCodec.cpp:969-1446` | 478 |
| `validateDocument` | `src/io/serializer/DocumentValidation.cpp:84-488` | 405 |
| `LayerDock::connectControls` | `src/ui/LayerDock.cpp:269-605` | 337 |
| `DocumentSerializer::fromJson` | `src/io/DocumentSerializer.cpp:710-1018` | 309 |
| `WobblePopoverPanel` 생성자 | `src/ui/WobblePopoverPanel.cpp:61-353` | 293 |
| `renderLayerHierarchy` | `src/render/engine/LayerHierarchyCompositor.hpp:71-332` | 262 |
| `SettingsDialog` 생성자 | `src/ui/SettingsDialog.cpp:110-368` | 259 |
| `StrokeCoverageRenderer::render` | `src/render/StrokeCoverageRenderer.cpp:902-1145` | 244 |
| `GifWriter::write` | `src/io/GifWriter.cpp:505-746` | 242 |
| `duplicateStrokes` | `src/document/DocumentControllerStrokes.cpp:405-637` | 233 |
| `CanvasWidget::tabletEvent` | `src/ui/CanvasWidgetEvents.cpp:738-939` | 202 |
| `transformStrokes` | `src/document/DocumentControllerStrokes.cpp:750-951` | 202 |
| `activeStrokePreview` | `src/ui/CanvasWidgetPreview.cpp:222-419` | 198 |
| `CanvasDisplayWindow::renderFrame` | `src/ui/CanvasDisplayWindow.cpp:339-525` | 187 |

- **개선**: `strokeFromJson`은 mode별 파서로, `validateDocument`는 레이어/획 검증으로 나눈다. `connectDrawingToolSettings`(`MainWindowSettings.cpp:217-363`)는 거의 같은 람다 22개라 테이블로 바꿀 수 있다. `.clang-tidy`에 `readability-function-size`를 현재 최대치부터 시작하는 ratchet로 넣는다.

#### Q-06 · 낮음 · 중복 유틸, 매직 넘버, 암묵 결합

| 항목 | 위치 | 개선 |
|---|---|---|
| 캔버스 크기 범위 검사가 12개 파일 69곳 | `DocumentJsonCodec.cpp:511-514,647-650,783-786,1678-1681`, `DocumentSerializer.cpp:772-775`, `DocumentValidation.cpp:90-93,230-233`, `WawaV10Reader.cpp:150-156`, `FrozenFillMask.cpp:22-28` 등 | `DocumentLimits::isValidCanvasSize(QSize)` |
| 마스크 임계값 `128`이 14개 파일 약 47곳 | `StrokeMask`, `SelectionOperation`, `LayerOperationReplay`, `FloodFillMask` 등 | `constexpr uchar maskThreshold`, `isMasked(uchar)` |
| 프레임 정규화 `((f % n) + n) % n` 12곳 | `LayerHierarchyCompositor.cpp:133,164,215`, `RenderEngine.cpp:210,349`, `StrokeRenderer.cpp:466` 등 | 공용 함수 하나 |
| qCompress 헤더 파싱 4곳 | `DocumentJsonCodec.cpp:529-534,669-674,819-824`, `BoundedCompression.cpp:35` | `BoundedCompression` API로 통일 |
| `setError`/`fail` 헬퍼 8벌 | `SerializerSchema.hpp:78`, `RecoveryStore.cpp:27`, `ApplicationInstanceLock.cpp:30`, `WawaV10Reader.cpp:30` 등 | 공용 오류 타입 |
| 커버리지 경계 `1.025 * 2.1 + 4.0`이 다른 파일 상수에 암묵 의존 | `StrokeCoverageRenderer.cpp:309-310` ↔ `StrokeRenderer.cpp:1148`, `StrokeMotionModel.cpp:145`, `ClassicStrokeMotion.cpp:44` | 모션 상수를 한 헤더로 모으고 경계를 그 상수에서 계산 |
| 이름 없는 상수 | 자동저장 `30000`(`MainWindow.cpp:236`), undo 한도 `64`(`DocumentController.cpp:389`), 개미 타이머 `120`(`CanvasWidget.cpp:172`), 점 간격 `0.75`(`CanvasWidgetTools.cpp:131,182`), 휠 줌 `1.0015`(`CanvasWidgetEvents.cpp:733`), JPEG 품질 `92`(`ExportWorker.cpp:252`), 로그 `2 MiB×3`(`Logging.cpp:75`) | 이름 붙은 `constexpr`. `CanvasViewport.hpp:25-33`처럼 |
| Theme을 거치지 않는 `QColor` 리터럴 약 17곳 | `CanvasViewport.cpp:26-31,105-112`, `ColorPairSwatch.cpp:56,103`, `CanvasWidgetEvents.cpp:374,407,409` 등. 체커 색은 셰이더(`canvas_frame.frag:22`)와 이중 정의 | Theme 경유 |
| 거의 같은 위젯 쌍 | `BrushPresetButton`/`EraserPresetButton`(카드 페인트 약 60줄), `BrushSizeRow`/`StrokeStabilizationRow`, 브러시·지우개 설정자 12쌍(`CanvasWidget.cpp:237-298,1058-1188`) | 공용 `SliderSpinRow`, 템플릿 |

#### Q-07 · 낮음 · 낡은 주석과 API 위생
- `src/app/MemoryBudget.hpp:11-14`는 "내보내기 전에 캐시를 비운다"고 하지만, 실제 `releaseTransientCaches`(`DocumentController.cpp:412-416`)는 컨트롤러 캐시만 비운다. `StaticLayerCache`와 저장·복구 스레드 캐시는 비우지 않는다(P-10).
- `src/render/engine/LayerOperationReplay.hpp:23-29`는 스케일 파라미터가 미리보기용이 아니라고 하지만, `DisplayScaleReplay.cpp:288-296`이 그 파라미터로 호출한다.
- `DocumentDelta.cpp:172-175`는 "주소로 판정"이라고 하지만, `:231`은 `fillCoverage ==`로 내용을 비교한다.
- `cmake/UguruguTargets.cmake:37` 주석이 삭제된 `CanvasFrameView`를 가리킨다.
- `CanvasDisplayWindow::grabFramebuffer`(`CanvasDisplayWindow.hpp:44`)는 테스트 전용인데 공개 API다.
- `src/document`에는 로그 호출이 한 건도 없다. 모든 reject가 흔적 없이 `false`로 끝난다(B-11).

---

## 4. 버그 및 잠재적 문제

먼저 확인한 양호한 부분을 적는다. 메모리 소유권 규율이 좋다. `connect`와 `singleShot` 람다는 모두 컨텍스트 객체를 갖는다(UI 전수 정규식 스캔에서 위반 0건). 워커 람다는 `this`를 캡처하지 않는다. 소멸자는 취소 토큰을 세운 뒤 풀을 대기한다(풀 멤버가 watcher보다 먼저 파괴되도록 선언 순서를 맞춤, `CanvasWidget.hpp:577-578,598-599`). 영역 안의 모든 `QImage` 생성은 `isNull()`을 검사하고, 바이트 계산은 `quint64`와 포화 산술을 쓴다. **문제는 예외·실패 경로와 상태 경계에 있다.**

### 4.1 높음

#### B-01 · 높음 · 바로 위가 클리핑 레이어면 '아래 레이어와 병합'이 결과 픽셀을 바꾼다
- **위치**: `src/document/DocumentControllerLayers.cpp:461-468`(`mergeLayerDownStatus`는 source와 target 자신의 `clipToLayerBelow`만 검사), `:504-544`(`mergeLayerDown`) / 클리핑 의미 `src/render/engine/LayerHierarchyCompositor.hpp:213-243`(클립 레이어는 직전 비클립 레이어 이미지를 기준으로 `DestinationIn`), `:280-290`
- **문제**: 레이어가 아래부터 [T][S][C(clip)]일 때 S를 T로 병합하면 C의 기준 알파가 S에서 T∪S로 넓어진다. T에만 내용이 있는 영역에 C가 새로 나타난다.
- **영향**: 병합은 픽셀을 바꾸지 않아야 하는데 그 불변식이 깨진다. 테스트(`tests/LayerCommandTests.cpp:655-693`)는 클립이 없는 경우만 고정한다. undo로 되돌릴 수 있지만, 사용자가 바로 알아채지 못하면 그대로 저장된다.
- **개선**:
  ```cpp
  // mergeLayerDownStatus: 프로퍼티 검사 직후
  for (int i = sourceIndex + 1; i < current.layers.size(); ++i)
  {
      if (current.layers[i].parentGroupId != source->parentGroupId)
          continue;
      if (current.layers[i].clipToLayerBelow)
          return MergeLayerDownStatus::UnsupportedProperties; // 전용 상태와 툴팁 권장
      break;
  }
  ```
  `LayerDock.cpp:820-822` 툴팁을 갱신하고, 클립이 위에 있는 병합 전후의 렌더 픽셀 비교 테스트를 추가한다.
- **근거**: 직접 확인(검사 코드와 컴포지터 의미 대조). 재현 테스트는 없다 · **관련 ID**: 없음

#### B-02 · 높음 · 렌더 캐시의 대기 표시가 예외에 안전하지 않아 영구 대기가 생긴다
- **위치**: `src/render/engine/StaticLayerCache.cpp:169-179`(pending 삽입 → 락 해제 → `render()` → 재잠금), `:143-152`(대기자는 20 ms 폴링 루프, 취소 토큰이 없으면 무한) / `src/render/RasterAssetCache.cpp:97-103`(in-flight 키 삽입), `:101`(타임아웃 없는 `wait`), `:106`(`compute()`), `:113-114`(제거·`wakeAll`) / 예외를 삼키는 쪽 `src/app/WatchedFutureResult.hpp:11-27`
- **문제**: 소유 스레드가 `render()`/`compute()` 안에서 예외(대표적으로 `std::bad_alloc`)를 던지면 pending 항목과 in-flight 키를 정리하는 코드가 없다. RAII도 try/catch도 없다. 워커 예외는 `watchedFutureResult`가 삼키므로 앱은 계속 돌고, 캐시만 오염된 채 남는다.
- **영향**: 같은 레이어와 소스(또는 같은 자산 id)를 요청하는 이후 모든 렌더가 영원히 기다린다. 취소 토큰이 `nullptr`인 호출자는 무한 루프다. GUI 동기 렌더(`CanvasWidgetPreview.cpp:147-176`)면 **UI가 멈추고**, 내보내기(`ExportWorker.cpp:215,293-298`)면 작업이 끝나지 않는다. `clear()`로도 pending은 지워지지 않는다(`StaticLayerCache.cpp:222-239`). 메모리 압박에서만 발생하지만, 4K·다레이어·8워커 조건이면 현실적이다.
- **개선**:
  ```cpp
  struct PendingGuard {
      State &cache; Key key; quint64 owner; bool committed = false;
      ~PendingGuard() {
          if (committed) return;
          QMutexLocker lock(&cache.mutex);
          auto it = cache.entries.find(key);
          if (it != cache.entries.end() && it->owner == owner && !it->ready)
              cache.entries.erase(it);
          cache.settled.wakeAll();
      }
  };
  ```
  `RasterAssetCache`에도 같은 형태의 `InFlightGuard`를 둔다(소멸자에서 키 제거와 `wakeAll`). 취소할 수 없는 대기에는 상한을 두고, 넘기면 직접 계산으로 빠진다. 소유자 예외, 소유자 취소, `clear()` 중 pending을 다루는 테스트를 추가한다(현재 `RasterAssetCache` 테스트 0건).
- **근거**: 직접 확인 · **관련 ID**: 없음(`346209e`의 in-flight 도입이 만든 지점)

#### B-03 · 높음 · 확대 중 캔버스 그림자 pixmap을 뷰포트가 아니라 캔버스 전체 크기로 만든다
- **위치**: `src/ui/CanvasWidgetEvents.cpp:287-333`(`refreshCanvasShadow`: `canvasPolygon.boundingRect() + 17 px`를 그대로 `QPixmap(bounds.size() * ratio)`로 생성), 호출 `:349-366`(노출 영역이 캔버스 밖에 걸리면 호출), 다각형 정의 `:340-343`(문서 전체 사각형을 화면 변환), `src/ui/CanvasViewport.hpp:28`(`maximumZoom = 16`)
- **문제**: 뷰포트 클립이 없다. 확대한 채 가장자리가 보이도록 팬하거나 회전하면 bake가 일어난다. 줌이 바뀔 때마다 다시 만든다. 할당이 실패해 null이 되면 캐시가 적중하지 않으므로 매 paint마다 재할당을 시도한다. GPU 경로도 `paintOverlay`를 그대로 쓰므로 똑같이 영향을 받는다.
- **영향**: 2048×1536 문서를 400%, DPR 1.5로 보면 약 12.3K×9.3K px, 즉 **약 457 MB를 단번에 할당하고 투명으로 채운 뒤 AA 14패스로 그린다**(계산값, 미측정). macOS Retina(DPR 2)는 약 0.8 GB다. 최대 배율(1600%)에서는 수 GB 할당을 시도한다. 휠 줌 한 칸마다 GUI 스레드가 이 작업을 한다.
- **개선**: bake 영역을 `bounds ∩ (rect() + margin)`으로 줄이고 캐시 키에 클립 원점을 포함한다. 또는 면적이 뷰포트의 4배를 넘으면 캐시 없이 `setClipRegion(shadowRegion)` 상태에서 직접 그린다. `QRegion(canvasPolygon.toPolygon())`(`:349`)도 뷰포트와 교차한 다각형으로 계산한다. 4096² 문서, 400~1600%, 회전 조건에서 그림자 캐시 크기 상한을 검사하는 테스트를 둔다.
- **근거**: 직접 확인(코드), 크기는 계산값 · **관련 ID**: 없음

### 4.2 중간

#### B-04 · 중간 · 그리는 중 합성 프레임이 최종 렌더와 최대 ±2 다른데, 펜을 떼면 '정확한 프레임'으로 캐시된다
- **위치**: `src/render/RenderEngine.cpp:296-313`(위쪽 레이어 `above`를 따로 평탄화), `:531-553`(`composeLayerSplit`: below → layer → above 순으로 source-over), `:573-591` / 사용처 `src/ui/CanvasWidgetPreview.cpp:122`(`activeStrokePreview`) / 승격 `src/ui/CanvasWidgetTools.cpp:191-238`(승격 프레임을 `m_frameCache`에 넣음) / 테스트 허용 오차 `tests/LayerSplitPreviewTests.cpp:255-268`(최종 `render()`와의 비교에서 채널 차 ≤ 2 허용)
- **문제**: 8비트 premultiplied에서 `(A over B) over C`와 `A over (B over C)`는 반올림 때문에 결과가 다르다. 서브 에이전트가 무작위 반투명 픽셀로 시뮬레이션했더니 최대 차이 2가 나왔고, 테스트의 허용 오차와 일치한다. `RenderPreviewTests.cpp:87`은 같은 경로끼리 비교하므로 사실상 동어반복이다.
- **영향**: 반투명 위쪽 레이어가 있으면, 그리는 중 화면과 승격돼 캐시에 남는 프레임이 워커가 다시 만든 정확한 프레임과 최대 2/255 다를 수 있다. 눈에 띄는 크기는 아니지만, 프로젝트가 핵심 계약으로 삼는 "미리보기 = 최종 픽셀 동일"과 충돌한다. 내보내기는 별도 렌더라 영향이 없다.
- **개선**: ① `above`를 평탄화하지 말고 위쪽 레이어 래스터를 유지해, 패치 영역에서 `renderLayerHierarchy`와 같은 순서로 레이어별 합성을 한다(`composeLayerRasterFrameRegion` 재사용). 비용은 패치 면적 × 위쪽 레이어 수다. ② 또는 above에 반투명 픽셀이 있으면 승격하지 않고 정확한 재렌더를 예약한다. ③ 테스트를 불투명(정확 일치 `QCOMPARE`)과 반투명(상한 명시)으로 나누고, 승격 프레임과 `render()`를 직접 비교하는 케이스를 추가한다. `promotesThePenUpFrameOfTheDisplayedDocument`에 반투명 위쪽 레이어 행을 넣는 것이 가장 빠르다.
- **근거**: 직접 확인(합성 순서, 테스트 허용 오차, 승격 캐시). 승격 프레임이 실제로 split 경로를 타는 조건에서 불일치가 생기는지는 **추측**이며, 위 테스트 행으로 확정해야 한다 · **관련 ID**: D02 인접

#### B-05 · 중간 · 선택 가시성 판정이 레이어별 우글거림 override를 무시한다
- **위치**: `src/document/SelectionVisibility.cpp:203-204`(프레임 수는 레이어 기준 `effectiveWobbleAmount`), `:234-245`(그런데 렌더에는 원본 `document`를 넘김) / `src/render/StrokeRenderer.cpp:441-449`(`document.wobbleAmount/motion`을 읽음) / 대조: `RenderEngineStrokes.cpp:406`, `StrokeCoverageRenderer.cpp:730`, `CanvasWidgetPreview.cpp:243`은 `documentForLayer`를 쓴다
- **문제**: 이 경로만 문서 수준 우글거림으로 기하를 만든다.
- **영향**: override가 있는 레이어(예: 멈춰 둔 배경)에서 "선택 영역에 보이는 픽셀이 있는가"가 실제 렌더와 다르게 판정된다. 가는 선이 선택 경계에 걸치면 이동, 복사, 삭제가 잘못 허용되거나 거부된다. 같은 이유로 `strokePreviewBounds`(`RenderEngineStrokes.cpp:164-194`, 테스트에서만 사용)와 레이어 썸네일(`LayerThumbnailRenderer.cpp:35-50`)도 override를 반영하지 않는다.
- **개선**: `RenderEngine::renderStrokesOnLayer*`가 `Layer`를 받아 내부에서 `documentForLayer`를 호출하게 한다. 호출자가 올바른 문서를 고르는 데 의존하지 않는 구조로 바꾼다.
- **근거**: 확인됨 · **관련 ID**: 없음

#### B-06 · 중간 · 펜이 닿은 채로 측면 버튼을 누르면 진행 중인 획이 말없이 버려진다
- **위치**: `src/ui/CanvasWidgetEvents.cpp:760-772`(`TabletPress`에서 `m_tabletSequence`가 true면 **버튼 종류와 무관하게** `cancelActiveInteraction()`을 호출하고, 비좌측 버튼이면 ignore), `:896-937`, `src/ui/CanvasWidgetTools.cpp:324-344`
- **문제**: 팁 접촉 중 측면 버튼 Press가 오면 획 전체가 취소된다. 이후 팁의 Release는 이미 시퀀스가 닫혀 무시된다. ignore된 이벤트는 Qt가 마우스 이벤트로 합성할 수 있다(D11).
- **영향**: 와콤류 펜에서 측면 스위치를 스치기만 해도 긴 획이 사라진다. 커밋 전이라 undo로도 되살릴 수 없다.
- **개선**:
  ```cpp
  if (event->type() == QEvent::TabletPress && m_tabletSequence
      && event->button() != Qt::LeftButton)
  {
      event->accept(); // 접촉 중 추가 버튼은 무시한다
      return;
  }
  ```
  Release도 팁(`LeftButton`)일 때만 시퀀스를 닫는다. 합성 `QTabletEvent`로 회귀 테스트를 만들고(`UiDrawingToolTests.cpp:208-263`의 방식), 인수인계 1번(실제 펜 확인) 때 함께 확인한다.
- **근거**: 코드 경로는 직접 확인. WinTab에서 Qt가 접촉 중 버튼 변화를 `TabletPress`로 전달한다는 점은 **추측** · **관련 ID**: D03, D11

#### B-07 · 중간 · 펜이 근접 범위를 벗어나면 Space 팬과 Shift 상태까지 초기화된다
- **위치**: `src/ui/MainWindow.cpp:736-739`(전역 필터가 `TabletLeaveProximity`에서 `cancelActiveInteraction()`), `src/ui/CanvasWidget.cpp:1659-1685`(특히 `:1681-1682`, `m_shiftPressed=false; setPanModifierActive(false)`), Space 자동반복 무시 `CanvasWidgetEvents.cpp:950-955`
- **문제**: 포인터 상호작용 취소와 키보드 수정자 초기화가 한 함수에 묶여 있다. Space는 자동반복을 무시하므로 계속 누르고 있어도 팬이 복구되지 않는다.
- **영향**: "Space를 누른 채 펜을 들어 위치를 옮기는" 흔한 팬 동작에서, 펜이 범위를 벗어나는 순간 팬이 풀리고 다음 접촉이 획이 된다.
- **개선**: `cancelPointerInteraction()`(획, 팬, 선택 이동)과 `resetKeyboardModifiers()`(FocusOut, WindowDeactivate, ApplicationDeactivate에서만)로 나눈다. 근접 이탈에서는 포인터만 정리하고, 호버 링 커서를 숨긴다(지금은 마지막 위치에 남음).
- **근거**: 확인됨 · **관련 ID**: D03(정책 결정에 "수정자 초기화 부작용"을 추가)

#### B-08 · 중간 · 종료가 진행 중인 업데이트 확인·다운로드를 무기한 기다리고, 그동안 인스턴스 락을 쥔다
- **위치**: `src/app/BackgroundWork.cpp:13`(`QThreadPool::globalInstance()->waitForDone()`), `src/app/UpdateControllerWindows.cpp:168,311`(`QtConcurrent::run`, 전역 풀), `:399-404`(시작 2초 뒤 자동 확인), `src/main.cpp:308`(join), `:175`(락은 함수 끝까지 유지)
- **문제**: 업데이트 작업에 취소 수단이 없다. 종료 시 전역 풀 join이 그 작업의 완료를 기다린다.
- **영향**: 오프라인이거나 HTTP가 지연될 때 시작 직후 창을 닫으면 창은 사라졌는데 프로세스가 남는다. 다시 실행하면 창 없이 "Ugurugu is already running." 대화상자만 뜬다(`main.cpp:208-215`). 다운로드 중에는 다운로드가 끝날 때까지 종료가 막힌다. macOS(Sparkle)에는 해당하지 않는다.
- **개선**: 업데이트 작업을 전용 `QThreadPool`로 옮겨 전역 join에서 뺀다. 종료 시 인스턴스 락을 join보다 먼저 해제한다. 다운로드 진행 대화상자에 취소를 넣는다(`setCancelButton(nullptr)` 제거).
- **근거**: 직접 확인(호출 체인). Velopack의 HTTP 타임아웃 길이는 추측 · **관련 ID**: D18 보강

#### B-09 · 중간 · `.ugu` 열기: 구조 예산 검사 전에 JSON DOM이 부풀고, 열기 경로에 `bad_alloc` 처리가 없다
- **위치**: `src/io/DocumentSerializer.cpp:713-725`(128 MiB 바이트 검사 직후 `QJsonDocument::fromJson`), `:794`(개수 예산은 DOM 완성 뒤), `:863-864`(래스터 base64를 `toString()` → `toLatin1()` → `fromBase64()`로 복사) / `src/ui/MainWindow.cpp:281-303,305-370`(try/catch 없음) / `src/main.cpp:296-306`(이벤트 루프 밖의 catch만 있음)
- **문제**: 점 하나 `[0,0,0],`는 8바이트인데 Qt는 배열마다 컨테이너를 할당한다. 그래서 128 MiB 이하 입력이 수 GB DOM이 될 수 있다(추측, 미측정). 래스터 문자열 하나가 UTF-16, Latin1, 디코드 결과로 세 번 동시에 살아 있다. 예외가 나면 앱이 "예기치 않은 오류" 후 종료한다. 저장, 내보내기, `replaceDocument`만 예외를 처리한다.
- **영향**: 공유받은 파일 하나로 앱 종료나 스왑 폭주가 가능하다. 열기 전에 `maybeSave`를 거치므로 유실은 제한적이다.
- **개선**: `fromJson` 앞에 O(n) 사전 검사(`'['` 개수 ≤ 점 한도 + 획·레이어·자산 수의 상수배)를 둔다. base64 문자열 길이를 복사 전에 상한과 비교한다. `readProject`/`activateProject`에서 `catch (const std::bad_alloc&)`로 "메모리 부족" 안내를 띄운다. D04의 peak RSS 측정에 "점 배열 폭탄"을 추가한다.
- **근거**: 순서와 핸들러 부재는 확인됨. 증폭 배율은 추측 · **관련 ID**: D04

#### B-10 · 중간 · 편집과 히스토리 경로가 예외에 안전하지 않고, 매크로·preflight 정리가 RAII가 아니다
- **위치**: 미보호 `tryCommitCandidate`(`DocumentController.cpp:1190-1228`), `prepareState`(`:1308-1341`), `DocumentUndoStack::push/undo/redo`(`DocumentUndoStack.cpp:111-207`) / 수동 매크로 쌍 5곳: `CanvasWidget.cpp:725-729`, `CanvasWidgetText.cpp:237-252`, `LayerDock.cpp:1221-1235`, `MainWindow.cpp:1130-1137,1175-1181`
- **문제**: (a) `push`는 redo 꼬리를 먼저 버리고(`:119-127`) 나서 `command->redo()`를 실행한다(`:132`). redo가 던지면 꼬리만 사라진다. (b) undo 중 예외가 나면 `clearHistoryPreflight`(`:180`)를 건너뛴다. 그러면 `m_staged`가 남아 이후 preflight가 영구히 실패한다(`DocumentController.cpp:222`). (c) 매크로 begin과 end 사이에서 예외나 early return이 생기면 매크로가 열린 채 남는다. 이후 편집은 이력에 남지 않고 `isModified`도 false가 된다.
- **영향**: 메모리 압박 한 번으로 undo 기능 전체가 조용히 망가지거나, 수정 표시가 사라져 저장 확인 없이 종료될 수 있다(추측).
- **개선**: RAII `HistoryMacroScope`와 preflight `ScopeGuard`를 둔다. 커밋, undo, redo 진입점을 `guarded()`로 감싸 `bad_alloc`을 "실패 + false + 오류 신호"로 바꾼다. redo 꼬리는 새 명령의 redo가 성공한 뒤에 버린다.
- **근거**: 코드 구조는 확인됨. 발생 확률은 추측 · **관련 ID**: 없음

#### B-11 · 중간 · undo/redo 실패는 흔적 없이 조용하고, 불변식 위반은 릴리스 빌드에서도 `qFatal`로 프로세스를 죽인다
- **위치**: `src/document/history/DocumentUndoStack.cpp:161-207`(preflight 실패 시 그냥 `return`), `src/document/DocumentController.cpp:220-262`(node나 `compactSize`가 불일치하면 false), `:290-300`(`requireReady` → `qFatal`), `src/ui/MainWindowActions.cpp:266,277`(결과 없는 void 호출)
- **문제**: 일시적 prepare 실패는 재시도로 회복된다. 하지만 결정적 불일치는 그 항목에서 영구히 멈춘다. 이후 undo는 모두 막히는데 알림도 로그도 없다.
- **영향**: 현장 버그를 재현하거나 추적할 수 없다. 최악의 경우 미저장 작업을 자동저장 시점까지 잃고 앱이 즉시 종료된다.
- **개선**: `qFatal`을 경고 로그 + 히스토리 비우기 + 문서 유지로 바꾼다. 엔진용 `Q_LOGGING_CATEGORY`를 추가해 spdlog로 흐르게 한다. `undo()`/`redo()`가 결과를 반환하거나 `historyMovementRejected(QString)` 신호를 낸다.
- **근거**: 직접 확인(`qFatal` 지점) · **관련 ID**: D07

#### B-12 · 중간 · 백그라운드 저장 완료 처리가 방어적이지 않고, 실패 경로에 테스트가 없다
- **위치**: `src/ui/MainWindow.cpp:1018-1056`(`finishSave`, 특히 `:1026-1028`의 `resultCount() > 0 ? result() : SaveResult{}`), `:944-962`, `:848,950`(`waitForSave()`의 실패 반환을 무시), `:1008-1011,1035-1037`(예외 `what()`을 그대로 표시)
- **문제**: watcher가 실제로 끝났는지 확인하지 않는다. "결과 없음"을 곧바로 실패로 본다. 이전 저장의 늦은 `finished`가 새 저장 직후 도착하면 진행 중인 저장을 실패로 오판할 수 있다(추측, 창이 매우 짧음). `bad_alloc`이면 사용자에게 "bad allocation"이, 빈 오류면 "Could not save the project.\n\n"이 보인다. 같은 목적의 헬퍼 `watchedFutureResult`가 있는데 이 경로는 쓰지 않는다.
- **영향**: 디스크 가득 참이나 권한 오류처럼 가장 중요한 실패 경로에 회귀 보호가 없다. `saveToFile`이 false가 되는 테스트는 예약 경로 거절 하나뿐이다(`tests/UiSessionTests.cpp:153`).
- **개선**: `if (!m_saveWatcher.isFinished()) return;` 가드를 넣고 `watchedFutureResult` 패턴으로 통일한다. 예외를 번역된 사용자 문구로 매핑한다. 읽기 전용 폴더, 없는 폴더, 경로가 디렉터리인 경우를 넣고 "modified 유지, 복구본 유지, 원본 보존, 오류 표시"를 검증하는 테스트를 추가한다.
- **근거**: 구조는 확인됨. 늦은 이벤트 경합은 추측 · **관련 ID**: 없음(`50c7a2a` 신규 경로)

#### B-13 · 중간 · 복구 파일 수명주기의 허점
- **위치**: `src/app/RecoveryStore.cpp:339-387`(preserve, quarantine), `src/ui/MainWindow.cpp:525-547,582-623`, `:653-668`(`closeEvent`), `:236-239`(자동저장 타이머), `:729-735`, `src/main.cpp:287-291`, `src/app/RecoveryWriter.cpp:27-41`
- **문제**:
  - ① `recovery-preserved-*.ugu`와 `.failed-<stamp>`가 개수 상한이나 보존 기한 없이 쌓인다. 파일당 최대 128 MiB다.
  - ② "복구본 보존 후 파일 열기"에서 열기가 실패하면 앱이 `Failed`로 종료한다. 다음 실행 때는 프롬프트가 없고, 보존 파일의 위치도 안내되지 않는다.
  - ③ 복구 실패나 요청 파일 오류가 나면 빈 창으로 계속하지 않고 앱이 종료된다.
  - ④ `closeEvent`가 `clearAutosave()`를 한 뒤에도 타이머와 비활성 이벤트가 살아 있다. 그래서 "저장 안 함"으로 종료했는데 다음 실행에서 복구 프롬프트가 뜰 수 있다(추측, 재현 안 함).
- **영향**: 디스크 누적, 보존 데이터를 찾지 못하는 상황, 시작 불가, 유령 복구 프롬프트.
- **개선**: 보존과 quarantine 파일에 상한을 둔다. 실패 대화상자에 항상 경로를 표시한다. 시작에 실패하면 빈 창으로 계속하는 선택지를 준다. `closeEvent`에서 `m_autosaveTimer.stop()`과 종료 중 플래그로 `writeAutosave`를 막는다.
- **근거**: ①②③은 확인됨, ④는 추측 · **관련 ID**: D19

#### B-14 · 중간 · 선택 조작이 고정된 undo 64칸을 소비해 그리기 이력을 밀어낸다
- **위치**: `src/document/DocumentController.cpp:389`(`setUndoLimit(64)`), `:472-514`(`pushSelectionStateCommand`), `src/document/history/DocumentUndoStack.cpp:327-374`(개수 기준 퇴출), 동작을 고정한 테스트 `tests/DocumentHistoryTests.cpp:238-266`
- **문제**: 선택, 반전, 해제는 문서를 바꾸지 않는데도 일반 항목과 같은 개수 한도를 쓴다. 한도는 상수이고 사용자 설정이 없다. 바이트 예산(192 MiB)이 따로 있는데 개수 상한이 하나 더 걸려 있다.
- **영향**: 마술봉이나 올가미를 몇십 번 시도하면 앞선 그림 편집을 undo할 수 없게 된다. 선택 조작만으로 redo 꼬리도 버려진다.
- **개선**: 선택 항목은 개수 한도에서 빼거나 별도 한도를 둔다. 기본값을 이름 붙은 상수로 올리고 설정에 노출한다. 바이트 예산이 실질 상한 역할을 하게 한다.
- **근거**: 직접 확인 · **관련 ID**: 없음

#### B-15 · 중간 · QRhi 파이프라인이 해제된 `QRhiShaderResourceBindings`를 가리킨다
- **위치**: `src/ui/CanvasDisplayWindow.cpp:112-140`(`m_bindings.reset(newShaderResourceBindings())`로 교체), `:214,224`(파이프라인 생성 시 바인딩을 저장), `:295-298,400-407`(크기 변경 시 재생성), `:503-511`(주석으로 인지하고 `setShaderResources`에 현재 바인딩을 명시해 우회)
- **문제**: 프레임 텍스처 크기나 창 크기가 바뀔 때마다 바인딩 객체를 새로 만든다. 파이프라인이 저장한 포인터는 그 순간부터 dangling이다. Qt가 그 포인터를 다시 읽지 않는다는 구현 세부에 기대고 있다.
- **영향**: Qt 업그레이드나 검증 레이어에서 창 크기를 바꾸는 동안 크래시가 날 수 있다(추측).
- **개선**: 객체를 유지한 채 내용만 바꾼다(`m_bindings->setBindings({...}); m_bindings->create();`). 오버레이 바인딩도 같은 방식으로 한다. 변경이 작고 위험이 없다.
- **근거**: 포인터 교체는 확인됨. Qt의 접근 여부는 추측 · **관련 ID**: 없음

#### B-16 · 중간 · D3D11 device lost가 나면 복구 없이 세션 끝까지 소프트웨어 표시로 남는다
- **위치**: `src/ui/CanvasDisplayWindow.cpp:101-110`(`fail()` → `m_failed=true`), `:362-379`(`FrameOpDeviceLost`/`FrameOpError`), `:523`(`endFrame` 결과 무시), `:345-349`(재초기화 코드는 이미 있음), `src/ui/CanvasWidget.cpp:936-950`(로그는 항상 "GPU initialization failed", `:942`), `:904`(문서화되지 않은 환경 변수 `UGURUGU_CANVAS_DISPLAY=software`)
- **문제**: TDR, 하이브리드 GPU 전환, RDP 전환, 절전 복귀가 영구 폴백으로 이어진다. present에서 드러난 device removed는 다음 `beginFrame`까지 화면을 멈춘다.
- **영향**: 큰 창에서 그리기 지연이 이전 수준으로 돌아간다(최근 성능 작업의 효과가 사라짐). 원인을 구분할 수 없어 지원이 어렵다.
- **개선**: device lost면 리소스를 해제하고 `requestUpdate()`로 재초기화를 최대 N회 시도한다. `endFrame` 결과를 확인한다. 폴백 사유를 구분해 기록한다.
- **근거**: 코드는 확인됨. present 시 `DeviceLost`가 반환되는지는 추측 · **관련 ID**: 인수인계 2·4번 인접

#### B-17 · 중간 · GUI 스레드의 동기 I/O와 락 대기
- **위치**: `src/app/RecoveryWriter.cpp:57-66`(mutex를 쥔 채 파일 작업), `:230-241`(mutex를 쥔 채 `file.commit()`), `:43-55`(GUI의 `submitWrite`도 같은 mutex 사용) / `src/ui/MainWindow.cpp:1388-1389`(30초마다 GUI에서 `RecoveryStore::isRecoveryPath` 호출. 내부에서 `exists`, `canonicalFilePath`, `filesystem::equivalent`를 실행, `RecoveryStore.cpp:44-58`) / `:954-962`(`waitForSave()`가 무기한 대기)
- **문제**: 복구본 commit(fsync 포함) 동안 GUI의 `clearAutosave`나 `submitWrite`가 막힌다. 프로젝트가 네트워크 드라이브에 있으면 30초마다 경로 정규화가 GUI를 멈출 수 있다.
- **영향**: 느린 디스크나 끊긴 네트워크 경로에서 입력이 멈춘다(지속 시간은 추측).
- **개선**: 경로 비교는 `m_currentFilePath`가 바뀔 때만 계산해 캐시한다. commit은 mutex 밖에서 하고, generation을 commit 전후로 두 번 확인한다.
- **근거**: 확인됨 · **관련 ID**: 없음

### 4.3 낮음

| ID | 위치 | 문제 | 영향 | 개선 | 근거 |
|---|---|---|---|---|---|
| B-18 | `src/ui/CanvasWidgetEvents.cpp:479,487,590,663`, `CanvasWidget.cpp:1030-1040`, `:936-950` | `TabletRelease`가 유실되면 `m_tabletSequence`가 고착돼 마우스 좌클릭이 무시된다. Escape 분기에 이 플래그가 없고, 표시 창을 폐기할 때도 상호작용을 취소하지 않는다 | 드문 조건에서 마우스로 그릴 수 없다 | Escape 조건에 포함, `discardDisplayViews` 전에 취소, 타임스탬프 기반 해제 | 확인됨(유실 조건은 추측) |
| B-19 | `DocumentControllerLayers.cpp:194-204,1043-1100`, `LayerHierarchyCompositor.hpp:280-290` | 클립 레이어를 그룹으로 감싸거나 맨 아래로 옮기면 기준이 사라져 렌더에서 사라진다. 어떤 변경자도 정리하거나 안내하지 않는다 | 이유 없이 레이어가 안 보인다(의도된 규칙일 수 있음) | 감지해서 해제하거나 안내, 의도는 테스트로 고정 | 추측 |
| B-20 | `DocumentController.cpp:1447-1530` | undo(`Reverse`)에서도 이펙트를 정방향 순서로 발행한다 | 지금은 잠재 문제(비가환 이펙트 조합 없음) | `rbegin..rend`로 순회 | 확인됨 |
| B-21 | `WobblePopoverPanel.cpp:152-166` | 활성 레이어가 없을 때 "Active layer" 범위로 편집하면 문서 전체 값이 바뀐다 | 의도와 다른 범위가 조용히 바뀐다 | 대상이 없으면 패널 비활성 | 확인됨(도달 경로는 추측) |
| B-22 | `src/main.cpp:266-271` | `argv[1]`을 옵션 파싱 없이 무조건 파일로 취급한다. `--help` 같은 인자도 열기 실패 후 종료로 이어진다 | 낮음. Velopack 첫 실행과 재시작은 인자가 아니라 환경 변수로 신호하는 것으로 보여 해당 없을 가능성이 크다(추측) | `-`로 시작하는 인자 무시, 최소한의 `QCommandLineParser` | 확인됨 |
| B-23 | `TextStrokeBuilder.cpp:34-39,83`, `DocumentControllerStrokes.cpp:53-61` | 캔버스를 벗어난 글자 윤곽을 경계로 clamp한다. 채움은 정확히 잘리므로 둘이 어긋난다 | 경계에 걸친 글자에 가장자리 선이 생긴다 | 입력 점 범위를 완화하고 렌더에서 클립 | 추측(시각 영향) |
| B-24 | `CanvasWidgetTools.cpp:397-399` | `zoomToward`에 `isfinite` 가드가 없다(회전에는 있음, `CanvasWidget.cpp:1549`) | NaN이 들어올 경로는 아직 없다 | 가드 추가 | 확인됨 |
| B-25 | `DocumentSerializer.cpp:602-619` 외 `QSaveFile` 6곳 | `setDirectWriteFallback`이 없어 임시 파일을 만들 수 없는 폴더(ACL, 일부 공유)에서는 저장이 실패한다 | 특정 환경에서 저장 불가 | 실패 시 fallback으로 재시도 | 확인됨 |
| B-26 | `DocumentJsonCodec.cpp:406-450`, `:1298` | 마스크 id 조회가 실패하면 writer는 필드를 조용히 빼고, reader는 그 파일을 거절한다 | 저장은 성공하는데 다시 열 수 없는 파일이 생길 수 있다(경로는 추측) | writer에서 실패로 처리, 저장 후 구조 검증 | 확인됨 |
| B-27 | `ExportWorker.cpp:178-197` | `m_busy=false`를 큐드 `finished`보다 먼저 설정한다 | 좁은 창(모달 진행창 덕에 위험은 낮음) | 순서 교체 | 확인됨 |
| B-28 | `SelectionClipboardCodec.cpp:57-87,147-193` | 선택과 일부만 겹치는 획의 전체 기하와 이미지 자산 전체가 시스템 클립보드로 나간다 | 선택 영역만 복사했다고 믿는 사용자의 원본 노출(설계 선택) | 자산을 선택 bbox로 자르기, 안내 | 확인됨 |
| B-29 | `LayerHierarchyCompositor.hpp:216-223` | 캐시에서 나온 공유 `QImage`에 워커가 `QPainter`를 연다 | 드문 경합 가능성(추측) | painter 전에 `detach()` 명시 | 추측 |
| B-30 | `SelectionOutline.cpp:35-72` | 경계 변 전체를 해시에 보관하고 GUI에서 동기로 호출한다(`CanvasWidgetSelection.cpp:1095`). 테스트가 없다 | 노이즈가 많은 4096² 마술봉 선택에서 정지하거나 메모리가 폭증할 수 있다(추측) | 변 수 상한과 bbox 대체, 행 run 추적 | 확인됨(규모는 추측) |
| B-31 | `CanvasWidget.cpp:60` 대 `:919` | 생성자에서 설정한 BlankCursor가 나중에 만든 표시 창에 동기화되지 않는다 | 첫 표시 직후 잠깐 커서가 어긋날 수 있다 | 창 생성 직후 `setCursor(cursor())` | 확인됨 |

---

## 5. 성능

**전제**: 2026-10-03에 이미 큰 개선이 들어갔다. 정적 레이어 캐시, 워커 중복 제거, 펜 다운 split의 워커화, 색 기록 직접 그리기, 그림자 캐시, overlay 분리, 참조 이미지 선렌더, RasterAssetCache in-flight, 필압 체크포인트, 백그라운드 저장, 프레임 스트리밍 내보내기, `CanvasDisplayWindow` 도입(입력→프레임 p50 18.7 → 6.2 ms)이다. 이 설계는 유지해야 한다. 아래 항목은 **측정하지 않은 코드 구조 분석**이다. 기존 원칙대로 같은 조건 A/B로 측정한 뒤 고치기를 권한다.

**렌더링 루프와 타이머 비용 구조**: 재생은 `m_animationTimer`(PreciseTimer, `qRound(1000/fps)`, `CanvasWidgetPreview.cpp:1680-1686`) → `advanceFrame` → `QCache` 프레임 → 표시 순서로 돈다. 우글거리는 레이어는 프레임마다 처음부터 계산한다(검증 → 리샘플 → 아크길이 → 노이즈 변위 → `QPainterPath` → 래스터 → 마스크·Fill·Image 연산). 프레임 불변 레이어만 `StaticLayerCache`가 건너뛴다. 문서 한도(레이어 256, 획 20,000, 점 250,000, 한 변 4096, `DocumentLimits.hpp:14,40-45`) 때문에 획 수에 대한 O(N)은 상한이 있다. **레이어 수에 대한 초선형 비용과 4096² 표면 비용**이 더 중요하다.

#### P-01 · 중간 · 줌·DPR 변경 뒤 warmup이 다시 예약되지 않아, 재생 중 GUI 스레드가 프레임을 하나씩 동기 렌더한다
- **위치**: `src/ui/CanvasWidget.cpp:181-189`(줌 타이머는 `requestDisplayUpdate()`만 호출), `CanvasWidgetPreview.cpp:102-109`(렌더 크기가 바뀌면 캐시를 비움), `:172-177`(미적중 시 `renderScaled` 동기 호출), `:1579-1585`(워커가 크기 불일치를 보면 취소만 함), `:1722-1730`(`advanceFrame`의 "캐시된 프레임만" 가드는 warmup이 활성일 때만 동작). `scheduleFrameCacheWarmup()` 호출처는 `CanvasWidget.cpp:210,1434`, `CanvasWidgetPreview.cpp:1355,1397,1533`, `CanvasWidgetTools.cpp:260,338`뿐이다
- **문제**: 축소 표시(물리 배율 < 1)에서 줌이나 DPR이 바뀌면 렌더 크기가 바뀐다. 캐시는 비워지는데 warmup은 시작되지 않는다. 재생은 미적중마다 GUI에서 렌더하며, 이것이 한 바퀴(N 프레임) 동안 이어진다. `advanceFrame` 주석(`:1715-1721`)의 "GUI 스레드에서 렌더하지 않는다"는 의도와 어긋난다.
- **영향**: 재생 중 줌을 바꾸면 프레임당 렌더 시간만큼 입력이 한 바퀴 동안 막힌다. 사용자 문서 기준 약 60 ms × 프레임 수(기존 기록에서 유추, 추측). 휠로 연속 줌하면 반복된다. 기존 테스트(`defersPreviewRerenderUntilZoomInputIsIdle`, `UiViewportTests.cpp:1643`)는 재생을 끈 상태다.
- **개선**: 다음 프레임이 캐시에 없거나 stale이면, warmup이 없을 때 예약하고 반환한다.
  ```cpp
  if (!m_frameCache.object(nextFrame) || m_frameCacheStaleFrames.contains(nextFrame))
  {
      if (!m_frameCacheWarmupActive && !m_frameCacheWarmupScheduled)
          scheduleFrameCacheWarmup();
      return;
  }
  ```
  `DevicePixelRatioChange`도 처리한다. 검증은 `m_synchronousPreviewRenderCount`가 늘지 않는지로 한다(`rendersAnInvalidatedFrameOnceAndOffTheGuiThread` 패턴, `UiViewportTests.cpp:2498`).
- **근거**: 직접 확인(코드 경로). 체감 크기는 미측정 · **관련 ID**: D08

#### P-02 · 중간 · 선택 영역이 남아 있으면 120 ms마다 오버레이 전체를 다시 그리고 업로드한다
- **위치**: `src/ui/CanvasWidget.cpp:172-180`, `CanvasWidgetSelection.cpp:1098-1110,1241-1262`(`m_selectionMask`가 있는 한 타이머 동작), `CanvasDisplayWindow.cpp:287-337`(전체 클리어 후 `paintOverlay`, 전체 업로드)
- **문제**: 틱마다 dirty 영역이 위젯 전체다. 창이 숨겨지거나 최소화돼도, 유휴 상태여도 멈추지 않는다.
- **영향**: 1956×1148 창, DPR 1.5에서 틱당 약 20 MB를 클리어하고 업로드하며, 초당 약 8회다(계산값). 노트북 전력과 CPU를 쓰는데 눈에 보이는 효과는 점선 이동뿐이다.
- **개선**: 타이머 틱에는 선택 외곽선 bbox(+3 px)만 무효화한다. 창이 비활성이거나 숨겨지면 개미 애니메이션을 멈춘다.
- **근거**: 확인됨 · **관련 ID**: D21

#### P-03 · 중간 · 재생 타이머가 최소화·가림 상태에서도 돌고, 움직임이 없는 문서도 N 프레임을 렌더·보관·업로드한다
- **위치**: `src/ui/CanvasWidget.cpp:164-171`, `CanvasWidgetPreview.cpp:1680-1731`, 이벤트 처리는 `MainWindow.cpp:670-680,729-739`뿐(hide·최소화·`applicationStateChanged` 처리 없음) / `DocumentLimits.hpp:16,20`(최소 2프레임, wobble 0 허용), `CanvasWidget.hpp:515`(시작 시 재생 중), `CanvasWidgetPreview.cpp:1150`(프레임 수만큼 예산을 나눠 프리뷰 해상도를 낮춤)
- **문제**: 캔버스는 "애니메이션할 내용이 있는가"를 묻지 않는다. 빈 문서나 wobble 0 문서도 매 틱 다른 캐시 이미지를 골라 프레임 전체를 업로드하고(1400×1000 창에서 약 3 MiB, 기존 기록), warmup이 N 프레임을 보관한다. 대조적으로 `WobblePreview`는 show, hide, enabled에 맞춰 타이머를 관리한다(`WobblePreview.cpp:132-165`). 간격 `qRound(1000/24)=42 ms`는 실제 23.8 fps이고, 지연된 틱을 따라잡지 않는다.
- **영향**: 백그라운드 전력 소모. 정지 그림에서 불필요한 업로드와 메모리 사용. 큰 다프레임 문서는 프리뷰 해상도가 낮아진다.
- **개선**: 최상위 창의 `visibilityChanged`와 `applicationStateChanged`로 타이머를 정지하거나 재개한다(`isAnimating()` 표시는 유지). `documentVariesByFrame()`(모든 레이어에 `isLayerFrameInvariant` 적용 + motion 확인)이 false면 타이머를 켜지 않고 프레임 0만 보관한다. 재생은 경과 시간 기준으로 프레임을 고른다.
- **근거**: 확인됨 · **관련 ID**: 인수인계 "재생 중 텍스처 전체 업로드" 보류 항목

#### P-04 · 중간 · 펜업·undo 비용이 문서 크기와 이력 크기에 선형으로 늘어난다
- **위치**:
  - `src/io/DocumentSerializer.cpp:437,499-512`: `appendStroke`가 `PreparedPlan` 해시(획 N개)와 레이어 획 벡터를 COW로 분리하며 깊은 복사를 한다. `SerializerSchema.hpp:46-64`의 `StrokeMeta`는 `Stroke` 사본을 한 벌 더 들고 있다.
  - `src/render/RenderEngineStrokes.cpp:196-339`: `prepareRegionalStrokeRefresh`가 GUI에서 **모든 레이어**의 커버리지 계획을 다시 만든다. 마스크 bounds는 W×H를 스캔하고(`StrokeCoverageRenderer.cpp:166-196`) 호출 단위로만 캐시한다(`:738`).
  - `src/document/history/DocumentUndoStack.cpp:157,297-345`: push마다 보관된 이력 전체의 바이트를 처음부터 다시 센다. 공유 백킹마다 `QString` 키를 할당한다(`HistoryMemory.hpp:48-61`).
  - `src/document/DocumentControllerStrokes.cpp:205-216`: 소비자 없는 `StrokePresence`(Q-03)를 위해 획마다 캔버스 전체 마스크를 스캔하고 패킹하고 복사한다.
  - `src/document/DocumentController.cpp:233-253`: undo/redo preflight는 delta와 무관하게 전체 `prepare`를 다시 한다.
- **문제**: 기존 측정(pen-up p50 0.8 ms)은 252획 문서 기준이다. 문서 한도(20,000획)에 가까운 규모는 측정되지 않았다.
- **영향**: 큰 작업 문서에서 펜을 뗄 때와 undo 때 GUI 정지가 커진다(크기는 추측).
- **개선**: ① `StrokePresence` 제거(즉효, 안전). ② 이력 바이트는 백킹 주소를 키로 한 refcount 멀티셋과 누적 합계로 증감 계산. ③ 레이어별 커버리지 계획을 획 벡터의 d-pointer로 캐시. ④ `StrokeMeta`를 `{serializedBytes, maskId, backing}`으로 축소. 먼저 `ugurugu_stress_document_generator`로 20,000획 문서를 만들어 pen-up과 undo를 측정한다.
- **근거**: 구조는 확인됨. 크기는 추측 · **관련 ID**: project-status §5 "커버리지/합성 계획 반복", "획별 임시 할당"

#### P-05 · 중간 · 레이어 계층 분석을 레이어 쌍마다 새로 해서 split 준비가 O(L²~L³)다
- **위치**: `src/document/Document.cpp:160-171`(`isLayerDescendantOf`와 `layerDepth`가 호출마다 `analyzeLayerHierarchy`), `src/render/RenderEngine.cpp:128-137`, `:251-271`(`keepRoots`의 레이어 × 루트 이중 루프), `:193-195`(`renderLayerSplit`이 `supportsLayerSplit`을 다시 호출), `CanvasWidgetPreview.cpp:959` / 대조: `LayerThumbnailRenderer.cpp:30`과 `LayerDock.cpp:857`은 분석을 한 번만 한다
- **문제와 영향**: 평탄한 루트 레이어가 N개면 분석을 약 N²/2번 호출한다. 50개면 약 2,500회, 256개면 약 3만 회다(각각 해시와 벡터 할당 포함). 펜 다운과 warmup에서 실행된다(지연 크기는 추측).
- **개선**: `const auto hierarchy = analyzeLayerHierarchy(document);`를 한 번 만들고 `hierarchy.isDescendantOf(...)`를 쓴다. `supportsLayerSplit`과 `renderLayerSplit`이 같은 결과를 공유한다. 변경이 작고 결과가 바뀌지 않는다.
- **근거**: 확인됨 · **관련 ID**: 없음

#### P-06 · 중간 · 우글거리는 레이어는 프레임과 무관한 계산까지 매 프레임 반복한다
- **위치**: 기하 `src/render/StrokeRenderer.cpp:817-823`(`prepare`마다 새 `IncrementalGeometry`), `:600-684`(검증, 아크길이, 리샘플은 프레임 무관) / 클립 `src/render/engine/LayerOperationReplay.cpp:29-58,302-311,487`(`maskPath`가 W×H를 스캔해 행마다 `addRect`, 호출 단위 캐시, 획마다 `setClipPath`) / Fill `:179-220,249-292`(`fillCoverage->bounds`를 알면서 마스크 unpack과 픽셀 루프는 캔버스 전체, `SelectionOperation.cpp:502-534`) / Image `src/render/RasterAssetCache.cpp:141-178`(변환 결과를 **레이어 크기**로 만들어 128 MiB 디코드 캐시에 같이 저장. 4096²면 장당 64 MiB라 3장이면 LRU가 순환 미스) / 지역 렌더 폴백 `LayerHierarchyCompositor.cpp:330-361`은 정적 캐시를 거치지 않음
- **영향**: 선택 안에서 그린 획, 채우기, 붙인 이미지가 있는 큰 캔버스 문서에서 프레임 시간이 크다. Fill 하나에 프레임당 16.7 M 픽셀 루프와 16 MB 할당(계산값)이 든다.
- **개선**: 프레임 무관 기하(리샘플 점, 아크길이)를 `points.constData()`, 크기, 간격 키로 캐시한다. `maskPath`는 `mask.cacheKey()`로 프로세스 LRU에 두고 두 구현을 합친다. Fill 루프와 unpack을 bounds 안으로 한정한다. Image 변환 결과는 `(bounds, image)`로 저장하고 디코드 캐시와 분리한다. 각 변경은 `LegacyRenderGoldenTests`로 비트 동일함을 확인한다.
- **근거**: 구조는 확인됨. 크기는 계산과 추측 · **관련 ID**: project-status §5 "선택 clip path 재구성", "fill의 전체 캔버스 순회"

#### P-07 · 중간 · 연속 편집 중 warmup 폭풍: 편집마다 워커 8개를 만들었다가 취소한다
- **위치**: `src/ui/CanvasWidget.cpp:76-89`, `CanvasWidgetPreview.cpp:1364-1403,1423-1442,1507-1585`(취소된 generation 결과는 폐기), `CanvasWidget.cpp:195-198`(풀 우선순위를 설정하지 않음)
- **문제**: `invalidateFrames()`에 디바운스가 없다. 슬라이더를 드래그할 때처럼 편집 간격이 프레임 렌더 시간보다 짧으면 완료되는 프레임이 없다. 매번 `make_shared<Document>`와 watcher 최대 8개를 생성한다.
- **영향**: 연속 편집 중 프리뷰가 멈추고, 워커 8개가 GUI 스레드와 CPU를 다툰다(체감은 추측).
- **개선**: 현재 프레임만 인터랙션 워커로 즉시 처리한다. 나머지 warmup은 30~50 ms 트레일링 타이머 뒤에 시작한다. `m_frameCacheWarmupPool.setThreadPriority(QThread::LowPriority)`를 적용한다.
- **근거**: 확인됨 · **관련 ID**: D08

#### P-08 · 중간 · 모든 `documentChanged`가 선택을 비우고 전체 애니메이션의 선택 가시성을 다시 렌더한다
- **위치**: `src/ui/CanvasWidgetSelection.cpp:888-906`, `:327-354`(완료마다 상태바 메시지, `:348-352`), `:356-435`(전역 풀), `MainWindowActions.cpp:579-582`(평가 중 이동 모드 해제), 캐시 무효화 `DocumentController.cpp:625,1385,1683`
- **영향**: 선택한 채로 연속 작업하면 액션이 깜빡이고, 이동 모드가 풀리고, 메시지가 반복되고, 획마다 CPU 버스트가 생긴다. 전역 풀이라 warmup 제한도 받지 않는다.
- **개선**: 변경이 선택 레이어의 획 벡터에 닿았을 때만(`isSharedWith`) 재평가한다. 결과가 올 때까지 이전 선택을 유지하고, 결과가 같으면 메시지를 내지 않는다.
- **근거**: 확인됨 · **관련 ID**: D08

#### P-09 · 중간 · 취소가 연산 경계에서만 관찰된다
- **위치**: `src/render/engine/LayerOperationReplay.cpp:491-497,503,631`(일반 Paint/Erase만 있는 레이어는 마지막에 한 번만 확인), `DisplayScaleReplay.cpp:271-300,442`, `RasterAssetCache.cpp:101`(취소 없는 대기), `ExportWorker.cpp:215,238,286-298`(`renderScaled`에 취소 인자를 넘기지 않음)
- **영향**: 4K 다레이어 문서에서 취소나 중단 지연이 "레이어 하나 렌더 시간"만큼 남는다. 워커가 중단된 뒤에도 CPU를 쓴다. GUI 동기 렌더가 워커가 쥔 캐시를 기다리는 우선순위 역전이 생긴다.
- **개선**: `renderLayerStrokes`를 256획 단위로 나눠 그 사이에 확인한다. 내보내기에 취소 토큰을 연결한다. 캐시 대기에 취소와 상한을 추가한다(B-02와 함께).
- **근거**: 확인됨 · **관련 ID**: project-status §5 "취소 뒤 잔여 CPU·중단 지연" 남은 검증

#### P-10 · 중간 · 메모리 예산 회계의 공백 (D08 보강)
- **위치**: `StaticLayerCache` 정리 호출처 0건(`DocumentController.cpp:412-416`, `CanvasWidget.cpp:834-837`, `MainWindowExport.cpp:85-86,141-142`) / 엔트리가 옛 획 벡터와 `rasterAssets` 전체를 붙잡지만 예산에는 이미지 바이트만 계산(`StaticLayerCache.cpp:37-45,74,192`), 엔트리 파괴가 전역 mutex 안에서 일어남(`:70-77`), 문서 전체 `rasterAssets` 공유를 비교해서 이미지 하나만 붙여도 모든 정적 항목이 무효화(`:63-68`) / `SerializationCache`(각 64 MiB)가 컨트롤러, 저장 스레드, 복구 스레드에 3개인데 `static_assert`는 한 번만 계산(`MemoryBudget.hpp:43-60`) / 예산 밖의 표시 경로 표면: `m_lastDisplayedFrame`, 그림자 pixmap, 창 크기×DPR² 오버레이 이미지와 GPU 텍스처(`CanvasWidgetPreview.cpp:1218-1264`) / 도구 참조 이미지가 취소 후에도 watcher의 future에 남음(`CanvasWidgetTools.cpp:908-939`)
- **영향**: 문서를 닫고 다른 문서를 열어도 최대 수백 MiB가 회수되지 않는다. 장시간 사용 시 실제 상주 메모리가 문서화된 예산보다 크다.
- **개선**: `replaceDocument` 성공 직후와 내보내기 전에 모든 캐시(정적 레이어 포함)를 비운다. 엔트리는 락 밖에서 파괴한다. 정적 캐시 키에는 레이어가 쓰는 asset id 집합만 넣는다. `PreviewSurfaceUsage`에 표시 표면을 추가하거나 최소한 진단 로그를 남긴다. 취소 시 `setFuture(QFuture<QImage>())`로 결과를 놓는다.
- **근거**: 확인됨 · **관련 ID**: D08

#### 낮음

| ID | 위치 | 문제 | 개선 | 근거 |
|---|---|---|---|---|
| P-11 | `StrokeMotionModel.cpp:77-117,168-189` | Smooth/Stepped에서 샘플당 노이즈 32회 중, `randomness==0`(기본값)이면 8회가, 포즈가 같으면 절반이 결과에 쓰이지 않는다 | 비트 동일한 조기 분기 | 확인됨(이득은 추측) |
| P-12 | `Document.hpp:177`, `StrokeRenderer.hpp:69`, `LayerOperationReplay.cpp:628` | 기본 생성된 `Stroke`가 `QUuid::createUuid()`를 획 × 프레임마다 호출한다. `primitiveRun`이 획을 값으로 복사한다 | `m_identity`를 필요한 필드만 담도록 축소, 인덱스 구간 사용 | 확인됨 |
| P-13 | `StrokeRenderer.cpp:25-28,686-762`, `IncrementalStrokeRenderer.cpp:114-136` | Airbrush/Spray가 점 상한(dab 50,000)에 닿으면 갱신마다 전체를 다시 만든다(O(n²)). Spray는 입자 상한 뒤의 점을 조용히 그리지 않는다 | 상한 이후 고정 간격 유지, `visibleSegments` 증분화, 절단 시 알림 | 확인됨 |
| P-14 | `TimelineBar.cpp:55,66-79`, `MainWindow.cpp:274` | 전역 이벤트 필터가 이벤트 타입보다 부모 체인 순회를 먼저 한다. 태블릿 이동을 포함한 모든 이벤트에서 실행된다 | 타입 확인을 먼저, 또는 대상 위젯에 직접 설치 | 확인됨 |
| P-15 | `CanvasWidgetEvents.cpp:340`, `CanvasWidgetPreview.cpp:233,529` | `paintOverlay`와 `resolveDisplayedFrame`이 매번 `displayDocument()`로 `Document`를 복사한다. wobble OFF면 레이어 벡터까지 detach된다 | const 참조로 크기만 읽기 | 확인됨 |
| P-16 | `CanvasWidgetText.cpp:117-134,283-326` | 텍스트 배치 중 overlay paint마다 레이아웃을 두 번 다시 계산한다 | (텍스트, 폰트, 앵커) 키로 캐시 | 확인됨 |
| P-17 | `CanvasWidgetPreview.cpp:1136-1216`, `PreviewRenderPolicy.cpp:27-62` | 고배율에서 프리뷰가 문서 해상도 이하로 제한되고 뷰포트만 렌더하는 경로가 없다 | 정지 상태에서 뷰포트 고해상 모드(`renderScaledRegion`) | 확인됨(설계 제약) |
| P-18 | `WawaV10Importer.cpp:103-121`, `FrozenFillMask.cpp:41-77` | 구형 파일을 import할 때 채우기마다 캔버스 크기 마스크를 만든다(최대 20,000회). GUI에서 동기 실행된다 | bbox 크기 마스크, 워커로 이동 | 확인됨(시간은 추측) |
| P-19 | `ImageAffineTransformer.cpp:299-368`, `ImageResampler.cpp:168-190` | 비트 동일을 위한 16.16 고정소수 스칼라 루프다. 병렬화나 증분 계산이 없다 | 행 단위 병렬, 증분 역변환(비트 동일 테스트 필수) | 확인됨 |
| P-20 | `CanvasWidgetTools.cpp:129-135,176-189` | 최소 점 간격 0.75가 문서 단위 상수라서, 고배율에서 화면 3~12 px 미만 이동이 버려진다 | 화면 기준(`0.75 / zoom`)으로 | 확인됨(품질 영향은 추측) |

**태블릿 입력 지연**: 최근 측정(입력→present p50 3.8 ms, 대부분 vsync 대기)과 인수인계 5번의 결론에 동의한다. GUI 작업은 프레임당 약 0.4 ms라 렌더 스레드로 옮겨도 줄일 여지가 작다. PresentMon으로 present 이후의 실제 표시 지연을 먼저 재고, 그 결과로 frame latency waitable object나 "present 직전 최신 획 반영"이 필요한지 판단하는 순서가 맞다. 코드에서 새로 찾은 입력 지연 요인은 P-14(전역 필터)와 P-15(문서 복사) 정도이고, 영향은 작을 것이다.

---

## 6. UI/UX

### 6.1 관점별 요약

| 관점 | 현황 | 주요 이슈 |
|---|---|---|
| 첫 실행 | 시작 화면이나 온보딩 없이 1024×768 흰 캔버스가 바로 열리고, 애니메이션은 **이미 재생 중**이다. 하단에 회색 힌트 한 줄이 있다(`CanvasWidgetEvents.cpp:372-385`). 업데이트 확인은 2초 뒤 동의 없이 나간다 | U-06, U-11 |
| 도구 발견성 | 메뉴에는 단축키가 표시되지만 레일 버튼 툴팁에는 없다. 상태바 팁은 2곳뿐이다. 레이어 명령은 버튼 전용이고 Layer 메뉴가 없다 | U-07, U-12 |
| 단축키 | 표준 키는 `StandardKey`, 나머지는 `ShortcutBinding`으로 사용자 지정·중복 검사가 된다(좋음). Windows에서 Redo 별칭이 빠졌다 | U-02, U-13 |
| 실행 취소/다시 실행 | Edit 메뉴와 빠른 접근 툴바에 노출되고, enabled와 텍스트가 동기화된다. 히스토리 패널은 없다. 선택 조작이 한도를 소비한다 | B-14, U-17 |
| 펜 압력/타블렛 | 압력은 체크박스 하나(선형, 0.05~1 clamp)뿐이다. 지우개 끝은 감지한다(좋음). 진단은 로그에만 남는다 | U-09, B-06, B-07 |
| 고DPI | 델리게이트, 썸네일, 브러시 카드는 DPR을 인식한다(좋음). 아이콘은 DPR 1/2만 만든다. 캔버스는 DPR 변경 이벤트를 처리하지 않는다 | U-18, P-01 |
| 다국어 | ko/ja `.ts`는 727/727 완료이고 CI 게이트가 있다(좋음). **배포본에서 Qt 표준 문자열이 영어로 남는다** | U-01, U-14 |
| 접근성 | 이름을 의식적으로 붙인 곳이 많다(좋음). 키보드·보조기술 사각지대와 대비 부족이 있다 | U-07, U-08 |
| 미저장 종료 | `closeEvent` → `maybeSave`(저장/저장 안 함/취소) 흐름이고, 백그라운드 저장을 기다린다(좋음). 세션 종료, 내보내기 중 종료, 업데이트 실패 경로가 미비하다 | U-10, B-13 |
| 플랫폼 관례 | Settings·About 메뉴 역할을 지정했다. 파일 연결이 없고, Quit 역할이 없고, macOS 시스템 단축키와 충돌한다 | U-03, U-13 |

### 6.2 중간

#### U-01 · 중간 · 배포본의 한국어·일본어 UI에서 Qt 표준 문자열이 번역되지 않는다
- **위치**: `src/main.cpp:186-198`(접두 `"qtbase"`로 `QLibraryInfo::path(TranslationsPath)`에서 로드, 실패는 무시) / `cmake/UguruguPackaging.cmake:158-166`(deploy 스크립트는 카탈로그를 병합한 `qt_<lang>.qm`만 만든다) / **설치 트리 직접 확인**: `translations/`에 `qt_ko.qm`, `qt_ja.qm`만 있고 `qtbase_*.qm`은 없다
- **문제**: 개발 PC는 Qt 설치 폴더에 `qtbase_ko.qm`이 있어서 통과한다. 배포본에서는 로드가 실패한다. 어떤 테스트도 이를 검사하지 않는다(`tests/PackageSmoke.cpp`, `TestWindowsPackage.ps1`, CI).
- **영향**: 프로젝트 번역이 100%인데도 표준 버튼(OK/Cancel/Yes/No/Close/Restore Defaults), `QColorDialog` 전체, Qt 파일 대화상자 폴백, 텍스트 편집 컨텍스트 메뉴가 영어로 나온다.
- **개선**:
  ```cpp
  const QString path = QLibraryInfo::path(QLibraryInfo::TranslationsPath);
  if (!qtTranslator.load(interfaceLocale, u"qt"_s, u"_"_s, path))
      qtTranslator.load(interfaceLocale, u"qtbase"_s, u"_"_s, path);
  ```
  패키지 smoke에서 `translations/qt_ko.qm`이 있고 로드되는지 검사한다(macOS는 번들 내 경로).
- **근거**: 직접 확인(로드 코드와 설치 트리) · **관련 ID**: D15 인접

#### U-02 · 중간 · Windows에서 Redo가 Ctrl+Y뿐이고 Ctrl+Shift+Z가 동작하지 않는다
- **위치**: `src/ui/MainWindowActions.cpp:223-226`(`registerShortcut(redoAction, QKeySequence(QKeySequence::Redo))`). 별칭을 넘기는 선례는 ZoomIn(`:645-647`)이 있다
- **문제**: `QKeySequence(StandardKey)`는 플랫폼 바인딩 중 첫 번째만 가져간다. Windows의 Redo 바인딩은 "Ctrl+Y, Shift+Ctrl+Z"다. 같은 이유로 Cut/Copy/Paste의 보조 키가 빠지고, Windows에서는 Preferences와 Quit의 StandardKey가 비어 있어 단축키가 없다.
- **영향**: Photoshop, CSP, Krita, macOS 습관의 Ctrl+Shift+Z가 Windows에서 동작하지 않는다.
- **개선**: `const auto keys = QKeySequence::keyBindings(QKeySequence::Redo); registerShortcut(redoAction, keys.value(0), keys.mid(1));`. 단일 키와 충돌하는 별칭은 거른다.
- **근거**: 직접 확인 · **관련 ID**: D17

#### U-03 · 중간 · 파일 열기 진입점이 끊겨 있다 (파일 연결, 문서 타입, 두 번째 인스턴스)
- **위치**: `resources/macos/Info.plist.in`(`CFBundleDocumentTypes`/UTI 없음), `.github/workflows/release.yml:644-654`(`vpk pack`에 파일 연결 없음, 소스에도 등록 코드 없음), `src/main.cpp:208-215`(이미 실행 중이면 안내 후 종료, 파일 인자는 버림), `src/ui/FileOpenEventRouter.cpp:84-104`(문서 타입이 없으면 사실상 도달하지 않음)
- **영향**: `.ugu`를 더블클릭하거나 "연결 프로그램"으로 열 수 없다. 앱이 켜져 있으면 "이미 실행 중" 창만 뜬다.
- **개선**: plist에 문서 타입과 UTI를 선언한다. Windows는 Velopack 설치 훅에서 HKCU에 연결을 등록하고 제거 훅에서 해제한다. 두 번째 실행은 `QLocalSocket`으로 경로를 넘기고 기존 창을 raise한다(`FileOpenEventRouter`의 dirty 보호 재사용).
- **근거**: plist와 main은 확인됨. Windows 연결이 없다는 것은 grep 기준 · **관련 ID**: D09 구체화

#### U-04 · 중간 · 텍스트 도구: 배치 후 타이핑이 도구 전환과 재생 토글로 해석된다
- **위치**: `src/ui/CanvasWidgetEvents.cpp:455`(press마다 캔버스로 `setFocus`), `CanvasWidgetText.cpp:136-156`(편집기로 포커스를 넘기지 않음), `:319-321`(힌트는 "Text panel"인데 실제 도크 이름은 "Tool settings", `ToolDock.cpp:42`), `MainWindowActions.cpp:719,734,748-789`(단일 문자 WindowShortcut)
- **영향**: 캔버스를 클릭하고 글자를 치면 B/E/L/W/G/T/I/P/M이 도구 전환, 재생, 미러로 동작한다. 한글·일본어 텍스트를 넣으려면 반드시 마우스로 패널을 클릭해야 한다.
- **개선**: `textPlacementChanged(true)`를 받으면 `contentEdit->setFocus()`를 하고 ToolDock을 raise한다. 힌트에는 실제 패널 이름을 쓴다.
- **근거**: 확인됨 · **관련 ID**: 없음

#### U-05 · 중간 · 색 스와치의 "이전 색"이 드래그 직전 색이 아니라 직전 드래그 스텝 색이 된다
- **위치**: `src/ui/ColorWheel.cpp:421-436,477-479`(이동마다 `colorChanged`), `ColorDock.cpp:93-110` → `CanvasWidget.cpp:1047-1056` → `ColorPairSwatch.cpp:40-50`(호출마다 `m_previous = m_current`)
- **영향**: 휠을 한 번 드래그하면 previous가 거의 같은 색으로 덮인다. "뒤쪽 스와치 클릭 = 교환"(`ColorPairSwatch.cpp:20-21`)으로 직전 색을 되찾는 기능이 사실상 동작하지 않는다.
- **개선**: previous는 확정 시점(휠 mouseRelease에서 `colorCommitted`)에만 갱신한다.
- **근거**: 확인됨(시그널 체인) · **관련 ID**: R12 인접

#### U-06 · 중간 · 자동 업데이트에 동의나 옵트아웃이 없고, 결과가 작업 중 모달로 끼어든다
- **위치**: `src/app/UpdateControllerWindows.cpp:75-87,399-404`(2초 뒤 자동 확인), `:147-165,242`(자동 확인에서도 모달 `exec()`), `src/ui/MainWindow.cpp:729-735`(창이 비활성이 되면 획 취소), `SettingsDialog.cpp:110-329`(업데이트 설정 없음), `resources/macos/Info.plist.in:35-36`
- **영향**: 첫 실행부터 동의 없이 네트워크 요청이 나간다. 그리는 중에 결과가 도착하면 모달이 떠서 포커스가 옮겨지고, 진행 중인 획이 취소된다(D03 경로).
- **개선**: 설정에 "자동 업데이트 확인" 토글을 두고 첫 실행 때 고지한다. Sparkle의 `SUEnableAutomaticChecks`와 같은 값을 쓴다. 자동 결과는 비모달 배너나 상태바로 알리고, 그리는 중이면 미룬다. 업데이트 오류 문구는 번역한다.
- **근거**: 확인됨 · **관련 ID**: D18, D03

#### U-07 · 중간 · 키보드·보조기술 사각지대
- **위치**:
  - (a) 색 기록 버튼 256개를 항상 만든다(`ColorHistoryGrid.cpp:112-128`). 빈 슬롯도 접근성 트리에 노출된다(`:237-244`). 탭 정지 수는 Qt 기본 포커스 정책에 따라 최대 256개다(추측).
  - (b) 레이어 행의 눈과 우글거림 토글은 마우스 전용이다(`LayerItemDelegate.cpp:246-304`). 키보드는 Space로 가시성만 바꿀 수 있다(`LayerListWidget.cpp:55-65`). 우글거림 토글과 배지 "G/↳/R"에는 설명이 없다.
  - (c) 레이어 명령(추가, 복제, 병합, 삭제, 그룹, 이동)이 버튼뿐이라(`LayerDock.cpp:152-198`) 메뉴와 단축키 설정 목록에 없다.
  - (d) `CanvasWidget`과 `ColorWheel`에 accessibleName이 없다(`CanvasWidget.cpp:57`, `ColorWheel.cpp:100`).
  - (e) 스핀박스 증감 버튼을 폭 0으로 없앴다(`Theme.cpp:313-317`). 그래서 프레임 수, FPS, 줌, 브러시 크기를 펜·터치로 클릭해 조절할 수 없다. 도크 구분선은 3 px(`:359-363`), 스크롤바는 10 px(`:395-423`)로 타깃이 작다.
- **영향**: 키보드나 스크린리더 사용자는 레이어 조작과 색 선택을 할 수 없다. 펜 사용자는 숫자 조절이 불편하다.
- **개선**: 색 기록은 단일 탭 정지 + 화살표 이동(roving)이나 `QListView`로 바꾼다. 델리게이트에 키 경로와 `helpEvent`를 둔다. Layer 메뉴와 컨텍스트 메뉴를 만든다. 스핀 버튼을 복원하고 히트 영역을 키운다.
- **근거**: 코드는 확인됨, (a)의 포커스 기본값은 추측 · **관련 ID**: R12, D17(F2 이름 변경은 이미 지원됨, `LayerDock.cpp:147-149`)

#### U-08 · 중간 · 테마가 다크로 고정돼 시스템 설정과 고대비를 무시하고, 대비가 낮은 지점이 있다
- **위치**: `src/ui/Theme.cpp:180-222`(Fusion과 팔레트 전면 덮어쓰기), `MacWindowChrome.mm:30-31`(DarkAqua 강제), `colorScheme`·고대비 처리 0건
- **측정**(sRGB 상대휘도로 계산): 플레이스홀더 `#6A6F78` on `#2A2C30` = 2.77:1(`Theme.cpp:211`). 컨트롤 경계 `#3F434B` on `#24262B` = 1.53:1(`:100-103,296-304`, 비텍스트 3:1 미달). 빈 캔버스 힌트 `#9A9EA6` on 흰색 = 2.69:1(`CanvasWidgetEvents.cpp:374`). 본문 텍스트는 9.7~13.8:1로 양호하다.
- **영향**: 라이트 모드와 고대비 사용자를 지원하지 못한다. 첫 실행의 유일한 힌트가 흐리다.
- **개선**: 고대비 설정이면 시스템 팔레트를 쓴다. 경계와 플레이스홀더 색을 올린다. 힌트 색은 캔버스 배경과의 대비로 고른다. 라이트 테마는 장기 과제로 둔다.
- **근거**: 수치와 코드는 확인됨 · **관련 ID**: R13 확장

#### U-09 · 중간 · 펜 압력 설정과 진단이 부족하다
- **위치**: `src/ui/TabletPressureRow.cpp:16-50`(토글 하나, 브러시와 지우개가 공유), `CanvasWidgetTools.cpp:32-34`(선형, `[0.05, 1]` clamp), `src/main.cpp:76-114,235-239`(펜이 마우스로 들어오는 상태를 진단하지만 spdlog에만 기록), Help 메뉴에 로그 폴더 열기가 없음(`MainWindowActions.cpp:1071-1077`), `Logging::logFilePath()` 호출자 0건, WinTab이 무조건 켜지고 끄는 수단이 없음(`main.cpp:229`)
- **영향**: 필압이 먹지 않을 때 사용자가 UI에서 원인을 알 수 없고, 지원 요청 때 로그 위치도 찾을 수 없다. WinTab 드라이버 문제를 우회할 수단이 없다.
- **개선**: 입력 장치 상태(스타일러스 감지, 압력 지원)를 설정에 표시한다. "로그 폴더 열기"를 추가한다. 압력 곡선과 최소값을 제공한다. WinTab/Windows Ink 선택 옵션이나 환경 변수를 둔다. 도움말과 README에 안내를 넣는다.
- **근거**: 확인됨 · **관련 ID**: 없음

#### U-10 · 중간 · 종료와 세션 종료 처리의 빈틈
- **위치**: `src/ui/MainWindow.cpp:653-668`(`closeEvent`), `:844-918`(자체 저장 확인 대화상자, `:892-900`에 단일 키 S/N/Esc를 ApplicationShortcut으로 설정, 버튼 순서 고정 `:862-875`), `commitDataRequest`·`aboutToQuit` 사용 0건, `ExportWorker.cpp:31-40`(소멸자가 취소 후 대기, 종료 시 확인 없음), `UpdateControllerWindows.cpp:356-381`(업데이트 적용 실패 시 창을 다시 띄우는데, `closeEvent`가 이미 복구본을 지운 뒤다)
- **영향**: Windows 로그오프나 macOS 로그아웃 때의 동작이 Qt 기본값에 달려 있다(추측). 내보내기 중 종료는 고지 없이 취소된다. 업데이트가 실패하면 그 사이의 작업이 복구본 보호를 잃는다. macOS 관례(시트, "저장 안 함" 왼쪽)와 다르다. IME가 켜진 상태에서 S/N 단축키가 동작하는지 검증된 적이 없다.
- **개선**: `commitDataRequest`를 명시적으로 처리한다. 내보내기 중이면 확인한다. 업데이트가 실패하면 복구본을 다시 쓴다. 표준 버튼 역할의 `QMessageBox`나 macOS 시트를 쓴다.
- **근거**: 코드는 확인됨, 동작은 추측 · **관련 ID**: D03 인접

### 6.3 낮음

| ID | 위치 | 문제 | 개선 | 근거 |
|---|---|---|---|---|
| U-11 | `MainWindow.cpp:811-830`, `README.md:65-69`, `CanvasWidgetEvents.cpp:383`, `HelpDialog.cpp:29-30` | 첫 화면에서 Layers는 256칸 색 기록 뒤에, Wobble은 Tool settings 뒤에 숨는다. README는 "P를 눌러 재생"이라고 하지만 이미 재생 중이다(P는 일시정지). 도움말은 도구 7개 중 5개만 나열한다. README 단축키 표에 T, M, Ctrl+D, Ctrl+E, Ctrl+Shift+I가 없다 | Layers를 기본으로 raise, 도움말과 README를 액션 목록에서 생성 | 확인됨 · R14, D17, D25 |
| U-12 | `MainWindowActions.cpp:442,457,543,787,1114-1130`, `TimelineBar.cpp:89,144`, `CanvasWidgetEvents.cpp:382-383`, `MainWindow.cpp:700-702,865-872` | "(Enter)", "(Esc)", "(Ctrl+D)" 같은 단축키 표기가 하드코딩돼 있어 재할당하면 틀린다. macOS에서도 "Ctrl"로 표기한다. 레일 툴팁에는 단축키가 없다 | `shortcut().toString(NativeText)`로 생성하고 `changed`에서 갱신 | 확인됨 · D17 |
| U-13 | `CanvasWidgetEvents.cpp:382-383,456-461,776-778`, `Info.plist.in:5-6`, `MainWindowActions.cpp:157-160`, `MacWindowChrome.mm:17-37` | macOS: Ctrl+Space(→Cmd+Space) 줌 드래그가 Spotlight·입력 소스 전환과 충돌한다. `CFBundleLocalizations`가 없어 AppKit 문자열이 영어일 수 있다. Quit 메뉴 역할이 없다. DarkAqua를 창이 뜨기 전에 적용하려다 생략될 수 있다 | 충돌 없는 수정자, plist에 `en/ko/ja`, `QuitRole`, `showEvent`에서 재적용 | 코드는 확인됨, 영향은 추측 |
| U-14 | `RecoveryStore.cpp:103-384`(13곳), `RecoveryWriter.cpp:144-209`, `ApplicationInstanceLock.cpp:59,81` → 노출 `MainWindow.cpp:616-621,1461-1481` | 앱 계층 오류 문자열 약 20곳이 `QStringLiteral` 영어라 ko/ja 대화상자에 그대로 나온다 | 오류 코드 enum만 반환하고 UI에서 `tr()` | 확인됨 · D15 |
| U-15 | `MainWindow.cpp:172,1224-1229` | 다른 앱에서 이미지 붙여넣기와 파일 드롭이 안 된다(4초짜리 상태바 안내만 있음). 이미지 삽입에 단축키가 없다 | `QClipboard::image()`와 drop을 `insertImage`로 연결 | 확인됨 |
| U-16 | `SettingsDialog.cpp:450-492,151-154`, `ShortcutBinding.cpp:22-25` | "Restore Defaults"가 확인 없이 단축키, 언어, 테마, 폴더를 모두 초기화한다. 언어를 바꾸면 재시작해야 한다. `QLocale::setDefault`를 호출하지 않아 숫자 표기가 UI 언어와 어긋난다. 설정 키에 스키마 버전이 없고, objectName을 바꾸면 사용자 단축키가 사라진다 | 확인 대화상자, 설정 키 레지스트리와 버전, 액션 ID 불변 규약 | 확인됨 |
| U-17 | `MainWindowActions.cpp:99,151,162,994,231-255`, `MainWindow.cpp:793` | 니모닉 중복(&I 두 개, &S 두 개). `syncHistoryActions`가 번역된 "&Undo" 텍스트를 엔진 문자열로 덮어써서 니모닉과 번역이 사라진다. 최근 파일 목록과 전체 화면 단축키가 없다. 회전 버튼이 Undo/Redo 아이콘을 재사용한다 | 니모닉 정리, 히스토리 텍스트 번역 경로 수정, 최근 파일 | 확인됨 |
| U-18 | `Icons.cpp:638-650,655-677`, `WobblePlayButton.cpp:27-43`, `LayerItemDelegate.cpp:59-73` | 아이콘을 DPR 1.0/2.0으로만 만들어 125/150% 화면에서 보간된다. `WobblePlayButton`은 모니터를 옮겨도 다시 만들지 않는다. 강조색을 바꿔도 체크된 액션 아이콘이 갱신되지 않는다. 함수 정적 `QPixmap` 캐시가 강조색 변경마다 쌓인다 | `QIconEngine`이나 SVG 아이콘, `DevicePixelRatioChange` 처리, `Theme::accentChanged` 신호 | 확인됨 |
| U-19 | `SavePathDialog.cpp:38-62,93-101` | 내보내기 대화상자에서 JPEG 필터를 골라도 파일명이 `.png`면 PNG로 저장된다(접미사 우선, D05 정책의 결과) | `filterSelected`에서 이름의 접미사를 교체 | 확인됨 · D05 |

---

## 7. 빌드·배포

**현황 요약**
- **재현성**: 우수하다. FetchContent 5종을 URL과 SHA-256으로 고정했고, 배포 프리셋은 Qt 6.11.1과 정확히 일치해야 한다(`cmake/UguruguDependencies.cmake:22-30`). Actions `uses:` 29건이 모두 40자 SHA로 고정돼 있다.
- **CI** (`.github/workflows/ci.yml`): 정적 검사(clang-format, SPDX, 릴리스 메타), 번역(lupdate 경고=오류, unfinished 0), clang-tidy(경고=오류), 퍼저 4종 × 90 s, 빌드 매트릭스(Windows Debug, macOS ASan+UBSan, macOS Coverage에 라인 70% 게이트), 패키지(양 OS 배포 프리셋과 설치본 smoke)로 구성된다. 최상위 `permissions: contents: read`다.
- **릴리스** (`release.yml`): 태그와 CMake 버전 일치, 기본 브랜치 조상, 같은 SHA의 main CI 성공을 검증한다. macOS는 안쪽부터 hardened runtime 서명, 앱과 DMG 공증·스테이플, Sparkle EdDSA appcast, Gatekeeper, rpath, minos 감사, SHA256SUMS까지 한다. Windows는 `vpk pack`(vcredist 선행 조건), 격리 smoke(음성 대조군 포함), SHA256SUMS를 한다.
- **버전 관리**: 단일 소스는 `CMakeLists.txt:13`의 `project(VERSION 2.2.10)`이고, 여기서 `Info.plist`, `.rc`, About, vpk로 흐른다. 태그 31개는 CMake 버전과 일치하고, 릴리스 노트는 26개 버전 × ko/en/ja다. 태그는 모두 경량이고 서명이 없다.

#### R-01 · 중간 · 이미지 삽입이 광고하는 형식을 배포본이 디코드하지 못한다 (WebP/TIFF, macOS는 GIF도)
- **위치**: `src/ui/MainWindow.cpp:1608-1622`(필터 `*.webp *.bmp *.gif *.tif *.tiff`, 디코드는 `QImageReader`) / `ci.yml:166,254,326,390,472`, `release.yml:166,591`(Qt 모듈은 `qtshadertools`만 설치, `qtimageformats` 없음) / `cmake/UguruguPackaging.cmake:96-133`(macOS는 `qcocoa`, `qjpeg`, `qmacstyle` 세 플러그인만 수동 복사) / **설치 트리 직접 확인**: Windows `imageformats/` = `qgif`, `qico`, `qjpeg`, `qsvg`
- **문제**: 앱은 WebP를 내보내면서 같은 WebP를 삽입하지 못한다. 패키지 smoke는 JPEG만 검사한다(`tests/PackageSmoke.cpp:90-94`).
- **영향**: 사용자에게 보이는 기능이 모든 릴리스에서 부분적으로 깨져 있다. "디코드할 수 없음" 경고만 뜨고 데이터 손실은 없다.
- **개선**: CI와 릴리스의 aqt `modules`에 `qtimageformats`를 추가하고, macOS 수동 목록에 `libqgif`, `libqwebp`, `libqtiff`를 넣는다. 또는 필터를 실제 지원 형식으로 줄인다. 패키지 smoke에서 형식별 fixture를 디코드한다.
- **근거**: 직접 확인(Windows 설치 트리, macOS CMake 목록) · **관련 ID**: D10(원인 확정)

#### R-02 · 중간 · Windows 산출물에 서명이 없고, 자동 업데이트의 신뢰 앵커가 GitHub 릴리스뿐이다
- **위치**: `release.yml:644-658`(`vpk pack`에 서명 인자 없음), `src/app/UpdateControllerWindows.cpp:43-50`(`GithubSource`), `SECURITY.md:80-82`(서명 인증서 미구매를 "알려진 비용 트레이드오프"로 명시) / 대조: macOS는 Developer ID, 공증, Sparkle EdDSA(`Info.plist.in:39`)
- **문제**: SmartScreen 경고는 의도된 비용이다. 별개로 업데이트 무결성 문제가 있다. Windows 업데이트는 같은 출처의 피드와 패키지만 신뢰하고 독립된 서명 검증이 없다(Velopack의 해시 검증 범위는 추측).
- **영향**: GitHub 계정이나 토큰이 탈취되면 모든 Windows 설치에 정상 업데이트 경로로 코드가 배포된다. README가 "추가 정보 → 실행"을 안내하므로 사용자가 경고를 무시하는 습관을 갖게 된다.
- **개선**: OSS 대상 무료 서명(SignPath Foundation 등, 자격 확인 필요)이나 Azure Trusted Signing을 `vpk pack`의 서명 옵션에 연결한다. 그 전까지는 SHA256SUMS를 별도 채널로 서명해 게시한다. 계정에 2FA와 토큰 범위 최소화를 문서화한다.
- **근거**: 확인됨 · **관련 ID**: 없음

#### R-03 · 중간 · 서명·공증·Sparkle 비밀이 서드파티 액션·캐시와 같은 잡에서 쓰이고 승인 게이트가 없다
- **위치**: `release.yml:151-269`(macOS 잡 하나가 Qt 설치, 빌드, 테스트, 서명을 모두 수행), `:160-167,584-592`(`cache: true`, main CI가 만든 Qt 캐시를 복원), 비밀 사용 `:199-200,228,267-269,348-351,379`, `environment:` 없음
- **영향**: 캐시 오염이나 의존 도구 침해가 서명 키와 Sparkle 개인키(모든 사용자의 업데이트 신뢰 루트)로 이어질 수 있다. 액션이 SHA로 고정돼 있어 완화돼 있으므로 실제 악용 가능성은 낮다(추측).
- **개선**: 빌드·테스트 잡(비밀 없음)과 서명 잡(비밀 있음, 액션 최소)으로 나눠 artifact만 넘긴다. 서명 잡에 `environment: release`와 태그 제한을 건다. 릴리스에서는 Qt 캐시를 끈다.
- **근거**: 구조는 확인됨 · **관련 ID**: 없음

#### R-04 · 중간 · CI는 offscreen 테스트만 돌린다. GPU 표시 경로와 네이티브 대화상자는 CI에서 한 번도 검증되지 않는다
- **위치**: `cmake/UguruguTests.cmake:56`(`QT_QPA_PLATFORM=offscreen` 고정), `tests/UiViewportTests.cpp:2356-2360,2417-2421`(GPU 표시가 없으면 `QSKIP`), `docs/project-status.md`(windows 플랫폼에서 `configuresTheSaveDialogDefaultSuffix`가 간헐 실패)
- **문제**: 최근 그리기 지연 작업의 핵심인 `CanvasDisplayWindow`(QRhi, 입력 포워딩)가 로컬 수동 실행에만 기대고 있다. CTest는 SKIP을 실패로 보지 않는다.
- **영향**: 표시 경로와 Windows 네이티브 대화상자 회귀가 릴리스까지 갈 수 있다.
- **개선**: Windows 러너에서 `QT_QPA_PLATFORM=windows`(WARP D3D11)로 `ui_viewport`를 실행하는 잡을, macOS 러너에서 cocoa 잡을 추가한다. 이 잡에서 GPU 테스트가 0개 실행되면 실패로 처리한다. hosted runner에서 가능한지는 추측이다.
- **근거**: 직접 확인(로컬 실행에서도 2개 SKIP) · **관련 ID**: D24, D05

#### R-05 · 중간 · macOS 릴리스는 서명된 앱 자체를 한 번도 실행하지 않는다
- **위치**: `release.yml:402-454`(smoke 프로브 실행, `codesign`/`stapler`/`spctl` 검증만 함), `tests/PackageSmoke.cpp:45-47`(별도의 ad-hoc 서명 실행 파일), `release.yml:237-238`(엔타이틀먼트 없이 `--options runtime`) / 대조: Windows는 실제 `Ugurugu.exe`를 기동한다(`tests/TestWindowsPackage.ps1:148-181,304-306`)
- **영향**: hardened runtime의 library validation, 플러그인 서명 불일치, Sparkle 링크 문제처럼 서명 후에만 드러나는 로딩 실패가 공증과 배포를 모두 통과한 뒤 사용자에게 처음 보인다(가능성은 추측).
- **개선**: 서명과 스테이플 후 앱을 몇 초 실행해 살아 있는지 확인한다. smoke 종료 환경 변수를 두면 안정적이다. Metal 표시 초기화 성공 여부도 로그로 확인할 수 있다.
- **근거**: 확인됨 · **관련 ID**: D16, D06(Windows 한정)

#### R-06 · 중간 · Windows 전용 코드에는 정적 분석과 새니타이저가 없다
- **위치**: `ci.yml:311-348`(tidy는 macOS만), `cmake/UguruguBuildSettings.cmake:42-47`(MSVC 계열에서 sanitizer FATAL_ERROR), `src/app/UpdateControllerWindows.cpp`(414줄), `main.cpp`의 WinTab과 `Q_OS_WIN` 분기
- **영향**: Windows 빌드는 `-Werror` 컴파일만 거친다. Windows 전용 메모리 문제는 사용자가 발견하게 된다.
- **개선**: Ninja + clang-cl 프리셋으로 `compile_commands.json`을 만들어 tidy를 돌린다. 제한적으로라도 clang-cl `/fsanitize=address` 잡을 시도한다.
- **근거**: 확인됨 · **관련 ID**: D24

#### R-07 · 중간 · 네이티브 의존성의 취약점 추적이 수동이고, 버전이 여러 곳에 중복된다
- **위치**: `.github/dependabot.yml:3-5`("bumped by hand"), Qt 6.11.1 중복(`ci.yml:17`, `release.yml:17`, `CMakePresets.json:79,162`, `BUILDING.md:8,30,39,51`, `tests/LegacyRenderGoldenTests.cpp:76-78`), Velopack 라이브러리(`UguruguDependencies.cmake:120-122`)와 vpk 도구(`release.yml:19`), 버전 정규식 파싱 6곳(`ci.yml:56`, `release.yml:40,301,538,626,702`)
- **영향**: 신뢰할 수 없는 파일을 파싱하는 앱(libwebp, zlib, Qt imageformats)인데, 상류 보안 공지와 배포 사이의 지연을 측정하는 사람이 없다. Sparkle 취약점(D16)도 수동으로 발견했다. 버전을 올릴 때 6곳 이상을 고쳐야 하고 불일치 검사가 없다.
- **개선**: 예약 워크플로로 OSV-Scanner를 돌리거나 Renovate regex manager로 `FetchContent_Declare`를 추적한다. `cmake/UguruguVersions.cmake` 한 곳에서 버전을 정의하고 CI가 일치를 검사한다. SBOM(CycloneDX)을 생성한다.
- **근거**: 확인됨 · **관련 ID**: D16, project-status §6의 SBOM 후속

#### R-08 · 중간 · 퍼징이 얕다. 시드가 핵심 디코더에 닿지 않고 corpus도 남지 않는다
- **위치**: `cmake/UguruguFuzzing.cmake:34-63`, `ci.yml:283-309`(타깃당 90 s, `-max_len=65536`, corpus 캐시 없음, macOS만)
- **문제**: Wawa v10 퍼저의 시드는 Ugurugu JSON(`.wagle`)이다. 그런데 리더는 `{`로 시작하면 즉시 거절한다(`WawaV10Reader.cpp:382`). 그래서 바이너리 파싱은 처음부터 찾아야 한다. WWP 프리셋 퍼저의 시드는 프로젝트 파일(`examples/Wave.ugu`)이다. document/clipboard 퍼저의 유일한 시드는 schema 1이라 마스크, 래스터, `BoundedCompression` 경로에 닿으려면 변이로 schema 13을 찾아야 한다. 퍼저 환경에는 앱의 `QImageReader::setAllocationLimit(64)`(`main.cpp:173`)가 없다. 이미지 삽입과 복구 파일 읽기 퍼저는 없다.
- **영향**: "퍼저가 있다"는 인상보다 입력 경계 방어(D04)가 훨씬 약하게 검증된다.
- **개선**: 테스트 헬퍼로 schema 13 문서(마스크, 래스터, pixelSelection 포함), 유효한 WAWA 바이너리, `.wwpreset` 시드를 만든다. 딕셔너리를 추가하고, 야간 예약 잡(30분 이상)에서 corpus를 누적하며, 크래시 입력을 artifact로 올린다. `uncompressQtPayload`, `RasterAssetTable::registerPayload`, `RecoveryStore::load` 타깃을 추가한다.
- **근거**: 확인됨(도달 커버리지는 미측정) · **관련 ID**: D04, D24

#### R-09 · 중간 · 릴리스 런북과 비밀·키 관리 문서가 없다 (단독 유지보수자의 버스 팩터)
- **위치**: 문서 전체에 릴리스 절차, 필요한 비밀 7종(`MACOS_*`, `SPARKLE_PRIVATE_KEY`), 키 백업·교체 계획이 없다. `CONTRIBUTING.md:43-50`과 `BUILDING.md:93-137`에는 `ugurugu_tidy`, `ugurugu_format_check`, `ugurugu_package_smoke_test`, `UGURUGU_WARNINGS_AS_ERRORS`, sanitized/coverage/distribution 프리셋 설명이 없다.
- **영향**: Sparkle 개인키를 잃으면 기존 macOS 설치가 더 이상 업데이트를 받을 수 없다. 기여자는 CI 실패를 로컬에서 재현하기 어렵다.
- **개선**: `RELEASING.md`(버전 올리기, 노트 3종, 태그, 비밀 목록, 키 오프라인 백업과 교체 절차)를 쓴다. BUILDING에 프리셋과 타깃 표를 넣는다.
- **근거**: 확인됨 · **관련 ID**: D25

#### 낮음

| ID | 위치 | 문제 | 개선 | 근거 |
|---|---|---|---|---|
| R-10 | `ci.yml:31,330`, `.clang-tidy:12`, `release.yml:80-149`, `ci.yml:12-14` | clang-format(Xcode 번들)과 LLVM(brew, 패치 미고정) 버전이 고정되지 않았는데 `WarningsAsErrors: '*'`다. 릴리스는 같은 SHA에 대해 **데스크톱과 무관한 잡까지 포함한** CI 전체 성공을 요구한다. main push의 `cancel-in-progress`가 HEAD가 아닌 커밋 태그의 릴리스를 막는다 | 도구 버전 고정, 릴리스 필수 잡 목록 분리, 런북에 "태그는 HEAD에만" 명시 | 확인됨 |
| R-11 | `CMakePresets.json:41-43`, `Logging.cpp:49-100` | 릴리스 PDB/dSYM과 크래시 덤프가 없다. 로그는 info 레벨에 `flush_on(warn)`이라 크래시 직전 기록이 유실된다 | `/Zi`나 `-g`로 심볼을 만들어 artifact로 보관, `flush_every`, 최소 미니덤프 | 확인됨 |
| R-12 | `release.yml:796-800` | 초안 없이 릴리스를 만든 뒤 자산을 올리는데, 두 업데이트 경로가 모두 "latest"를 즉시 참조한다 | `draft: true`로 만들고 업로드 후 게시 | 추측(창의 영향) |
| R-13 | 설치 트리 | `opengl32sw.dll`(20.6 MB), `Qt6Network.dll`과 `tls/`, `networkinformation/`이 동봉된다. D3D11/Metal 표시와 Velopack 자체 HTTP에는 필요 없어 보인다 | windeployqt 옵션(`--no-opengl-sw` 등)으로 제외, 고지 범위 축소(L-01) | 직접 확인(필요 여부는 추측) |
| R-14 | `CMakeLists.txt`, `CMakePresets.json:3-7`, `BUILDING.md:5-6` | 프리셋 없이 단일 구성 제너레이터를 쓰면 빌드 타입이 비어 최적화가 꺼진다. 프리셋 최소 CMake(3.31)와 문서(Windows 4.2 이상)가 어긋난다 | 빈 빌드 타입이면 Release로, 최소 버전 정합 | 확인됨 |
| R-15 | 1.2절 | 구성 경고(zlib AUTOGEN), 링커 경고(`wWinMain`) | zlib을 가져올 때 AUTOMOC를 잠시 끄기(libwebp와 동일). 링커 경고는 무해하므로 문서화만 | 직접 확인 |
| R-16 | `git ls-files --eol`, `.gitignore` | `.gitattributes`와 `.editorconfig`가 없다(540개 파일이 작업 트리에서 CRLF). 루트의 개인 벤치 문서는 `.git/info/exclude`로만 가려진다. 저장소 URL과 저작권 표기가 여러 곳에 하드코딩돼 있다 | `* text=auto eol=lf`, `.gitignore`에 `/*.ugu` | 확인됨 |
| R-17 | 태그 31개, `release.yml:536-563,741-751` | 경량·미서명 태그, provenance attestation 없음, README에 체크섬 안내 없음 | 서명 태그, `attest-build-provenance` | 확인됨 |
| R-18 | `UguruguTools.cmake:144-215` | probe와 벤치마크 6개는 CI가 빌드하지 않아 링크·실행 부패를 잡지 못한다(컴파일은 tidy가 간접적으로 확인) | 월 1회 전체 타깃 빌드 잡 | 확인됨 |

---

## 8. 라이선스·저장소 관리

**GPL-3.0 준수 현황 (양호)**
- `LICENSE` 전문이 있다.
- src 261개, tests 54개, tools 7개 파일 모두 `SPDX-License-Identifier: GPL-3.0-or-later` 헤더가 있고 CI가 강제한다(`ci.yml:34-48`).
- `src/main.cpp:1-19`에 GNU 고지 전문이 있고, About 대화상자에 보증 부인과 소스 링크가 있다(`AboutDialog.cpp:32-46`).
- `CONTRIBUTING.md:5-21`에 inbound=outbound가 명시돼 있다.
- 대응 소스는 공개 저장소의 태그, 해시 고정 의존성, BUILDING.md로 제공돼 GPL §6(d)의 취지에 맞는다(법률 자문 아님).

**서드파티 점검표** (법률 자문 아님)

| 구성 요소 | 라이선스 | GPL-3.0+ 호환 | 고지 | 동봉 |
|---|---|---|---|---|
| Qt 6.11.1 (Core, Gui, Widgets, Concurrent, Svg 등) | LGPL-3.0, 동적 링크 | 호환 | `THIRD_PARTY_NOTICES.md:17-31`, `LGPL-3.0.txt`. **버전·모듈·소스 위치가 불완전(L-01)** | 양쪽 |
| Qt가 번들하는 서드파티(FreeType, HarfBuzz, libpng, libjpeg, PCRE2 등) | 각자(BSD/MIT/FTL/IJG 등) | 호환(추정) | **없음** | Qt 바이너리에 포함(추측) |
| Mesa llvmpipe(`opengl32sw.dll`) | MIT 계열 | 호환 | **없음(L-01)** | Windows(직접 확인) |
| `D3Dcompiler_47.dll` | Microsoft 재배포 조건 | 시스템 구성 요소 예외 해당 여부 검토 필요 | **없음(L-01)** | Windows(직접 확인) |
| spdlog 1.16.0(+번들 fmt) | MIT | 호환 | 있음 | 정적, 양쪽 |
| libwebp 1.6.0(+mux) | BSD-3 + 특허 허여 | 호환 | 있음(LICENSE와 PATENTS) | 정적, 양쪽 |
| zlib 1.3.2 | zlib | 호환 | 있음 | 정적, 양쪽 |
| Sparkle 2.9.6 | MIT(+번들 구성 요소) | 호환 | 있음 | macOS |
| Velopack 1.2.0 | MIT | 호환 | 있음. `Update.exe`/`Setup.exe`(Rust)의 크레이트 고지는 미확인(추측) | Windows |
| Pretendard JP v1.309(내장 OTF) | OFL-1.1(RFN "Pretendard") | 집합 저작물로 호환 | 있음. **파생 구성 요소 고지가 빠짐(L-02)** | 양쪽 |
| 앱 아이콘(seuppi) | GPL-3.0+ (README·NOTICES 선언) | 호환 | 선언은 있고 귀속 파일은 없음 | 양쪽 |
| MSVC 런타임 | Microsoft | 동봉하지 않음(Velopack 선행 설치) | 해당 없음 | — |

#### L-01 · 중간 · Qt(LGPL)와 동봉 바이너리 고지가 불완전하다
- **위치**: `THIRD_PARTY_NOTICES.md:17-31`("Qt 6"뿐, 정확한 버전·모듈·소스 위치 없음), `AboutDialog.cpp:42-44`(Qt 일반 문구, `QApplication::aboutQt` 없음), 설치 트리의 `opengl32sw.dll`, `D3Dcompiler_47.dll`(**직접 확인**, 고지 문서에 언급 0건)
- **영향**: LGPLv3 4항의 고지와 소스 제공 의무, 그리고 MIT(Mesa) 고지 조건을 충족하는지 모호해진다. project-status §6도 "artifact 기준 감사"를 후속 과제로 남겨 두었다.
- **개선**: NOTICES에 Qt 6.11.1과 사용 모듈, 소스 위치(Qt archive URL), Qt 번들 서드파티 목록을 적는다. Mesa와 D3Dcompiler를 고지하거나 동봉을 중단한다(R-13). About에 "About Qt" 버튼을 둔다. 설치본의 실제 파일 목록과 대조하는 체크리스트를 릴리스 런북에 넣는다.
- **근거**: 직접 확인(설치 트리, grep) · **관련 ID**: project-status §6 후속 감사

#### L-02 · 중간 · Pretendard JP 폰트의 저작권 귀속이 불완전하다
- **위치**: `resources/fonts/OFL.txt:1-2`(Kil Hyung-jin과 RFN "Pretendard"만), `THIRD_PARTY_NOTICES.md:53-58`
- **문제**: 폰트 name 테이블에 따르면 기본 글리프는 Inter, 한글·한자는 Source Han Sans/Noto Sans CJK, 가나는 M PLUS 1p에서 왔다. 각각 OFL-1.1이며 고유한 저작권 고지와 RFN("Inter", "Source")이 있다. 동봉 텍스트와 NOTICES에는 이들이 없다. 저작권 연도(2021)도 폰트 메타데이터(2023)와 다르다.
- **영향**: OFL 2항(번들 시 저작권 고지 동봉)이 일부 누락됐을 가능성이 있다(상류 원문은 미확인, 추측).
- **개선**: v1.309 상류 LICENSE 원문을 그대로 동봉하고, NOTICES에 파생 구성 요소 세 개를 열거한다.
- **근거**: 서브 에이전트가 폰트 메타데이터를 직접 읽음. 법적 중대성은 추측 · **관련 ID**: 없음

#### 낮음

| ID | 위치 | 문제 | 개선 | 근거 |
|---|---|---|---|---|
| L-03 | `tests/PackageSmoke.cpp:77-88,133-139`, `TestWindowsPackage.ps1:247-261` | 패키지 smoke의 필수 라이선스 목록에 `LGPL-3.0.txt`, `libwebp-LICENSE.txt`, `libwebp-PATENTS.txt`가 없다(설치는 됨) | 목록 추가 | 확인됨 |
| L-04 | `resources/icons/`, `THIRD_PARTY_NOTICES.md:67-71` | 아이콘 귀속·허락 기록 파일이 없다. Velopack 설치기 바이너리의 서드파티 고지는 미확인이다 | `resources/icons/ATTRIBUTION.md`, Velopack 고지 확인 | 추측 |

**저장소 문서 품질**

| 문서 | 평가 |
|---|---|
| README(ko/en/ja) | 세 언어의 구조가 같고(표 44행 동일) 설치, 기능, 단축키, 빌드 링크를 갖췄다. 실제 동작과 어긋나는 곳이 있다(U-11: 재생 기본값, 단축키 누락, 시작 화면). WinTab/Windows Ink 안내와 체크섬 검증 안내가 없다 |
| BUILDING.md | 기본 빌드는 정확하다. sanitized/coverage/distribution 프리셋, tidy/format/smoke 타깃, `UGURUGU_WARNINGS_AS_ERRORS` 설명이 없다(R-09) |
| CONTRIBUTING.md | 라이선스 정책은 명확하다. 로컬에서 CI 게이트를 재현하는 명령이 없다 |
| SECURITY.md | 범위와 지킬 수 있는 수준의 SLA를 정직하게 적었다(`SECURITY.md:41-55`). 언급한 "normal issue guide"에 해당하는 이슈·PR 템플릿이 없다 |
| docs/project-status.md | 단일 상태 문서로 ID 추적, 증거 범위, 측정 조건을 엄격하게 관리한다. 이 보고서가 그 형식을 따랐다 |
| 포맷 명세 | 없음(A-05) |

---

## 9. 테스트

### 9.1 현황

| CTest 스위트 | 클래스(슬롯 수) | 주로 다루는 모듈 | 실행 시간 |
|---|---|---|---|
| app | AppPolicy(8) | 인스턴스 락, 업데이트 간격 정책, FileOpenEventRouter | 0.1 s |
| document | Lifecycle 13, History 30, LayerCommand 31, Schema 19, Resize 11, StrokeCommand 8, SelectionClipboard 13, TextStrokeBuilder 6, SerializationBudget 24, RasterAssetTable 5, WawaV10Reader 6 | document/*, io/serializer/*, 복구 저장소, 클립보드, Wawa | 5.1 s |
| render | BrokenLine 7, ClassicMotion 4, RenderPreview 24, Wobble 11, LayerComposition 14, StrokeCoverage 17, LayerSplitPreview 13, LegacyGolden 2, FrozenFill 6, MotionTime 5, StrokeRendering 11, SelectionPreview 11, BrushRendering 5 | render/*, brush/* | 14.4 s |
| gif / webp / mask | 16 / 3 / 11 | 인코더, 내보내기 정책, 마스크 회귀 | < 1 s |
| release_notes / stabilizer | 5 / 6 | 릴리스 노트, 1€ 필터 | < 0.1 s |
| ui_shell / ui_selection / ui_viewport / ui_drawing_tools / ui_session | 62 / 55 / 50 / 28 / 22 | MainWindow, 선택·변환, 캔버스·표시, 도구, 복구·세션 | 2~10 s |

- 슬롯은 562개(UI 217, 엔진 345)이고, 실행 결과는 674건 통과에 2건 건너뜀이다. 테스트 코드는 약 31.2K줄로 src의 54%에 해당한다.
- **강점**: 임시 QSettings와 복구 경로로 격리한다(`tests/TestMain.cpp:47-65`). 대기는 `QTRY_*` 111회와 `qWaitForWindowExposed` 136회를 쓰고 고정 `qWait`는 9회뿐이다. 정확성 오라클(GPU와 software 비교, 증분과 전체 렌더 비교, 인-플레이스 패치 주소 검사)이 있다. 커버리지 70%가 게이트이고, 퍼저 4종이 있다.
- **직접 참조 테스트가 없는 모듈**: `Logging`, `UpdateController{Windows,Mac,Stub}`, `main.cpp`(시작, 설정 이전), `MacWindowChrome`, `RasterAssetCache`(0건), `SelectionOutline`, `CanvasDisplayWindow`(CI에서 SKIP). 대부분의 내부 클래스는 facade를 통해 간접적으로 검증된다.

### 9.2 이슈

#### T-01 · 중간 · 실패 주입 테스트가 없다 (저장, 복구, 열기, 메모리)
- **위치**: `tests/UiSessionTests.cpp:153`(saveToFile 실패는 예약 경로 거절 하나), `tests/UiShellTests.cpp:1381`(`bad_alloc`은 워커 하나)
- **영향**: B-09~B-13의 회귀 보호가 없다. 데이터 보존과 직결되는 경로다.
- **개선**: 읽기 전용 폴더나 파일, 없는 폴더, 디렉터리 경로, (가능하면) 작은 VHD로 디스크 가득 참 상황을 만들어 "modified 유지, 복구본 유지, 원본 보존, 안내"를 검증한다. `prepareState`에 `bad_alloc`을 주입하는 테스트 훅을 둬 커밋, undo, redo, 매크로 상태를 검증한다.

#### T-02 · 중간 · 캐시 실패와 동시성 테스트가 없다
- **위치**: `RasterAssetCache` 테스트 0건. `StaticLayerCache` 동시성 테스트(`tests/WobbleAnimationTests.cpp:173-196`)는 정상 경로만 본다. 렌더 중간 취소 테스트가 없다(`RenderPreviewTests.cpp:1328-1371`은 미리 취소된 경우만 다룸).
- **개선**: 소유자 예외, 소유자 취소 중의 대기자, `clear()` 중 pending, 같은 키 다른 소스 경쟁(B-02), 렌더 도중 취소 지연 상한(P-09)을 테스트한다. TSan은 Ugurugu 프레임만 읽는 전용 잡으로 운영한다(빌드 설정 주석 `UguruguBuildSettings.cmake:13-22`의 방침과 일치).

#### T-03 · 중간 · "미리보기 = 최종" 계약의 테스트 공백
- **위치**: `tests/LayerSplitPreviewTests.cpp:264-267`(±2 허용), `RenderPreviewTests.cpp:43-90`(DisplayPreview는 non-null과 통계만 확인)
- **개선**: ① split 합성을 불투명(정확 일치)과 반투명(상한 명시)으로 나누고, 승격 프레임과 `render()`를 비교한다(B-04). ② 획 종류별로 DisplayPreview와 NativeExact→축소 결과의 차이 상한을 골든으로 고정한다. 축소 표시 구간은 설계상 근사이므로 계약 문서에 "1:1 외에는 근사"를 명시한다. ③ 클립이 위에 있는 병합 픽셀 동일성(B-01). ④ 레이어 override와 선택 가시성(B-05).

#### T-04 · 중간 · 실제 펜 시나리오를 합성 이벤트로 재현하는 테스트가 없다
- **개선**: 기존 `QTabletEvent` 직접 전송 방식(`UiDrawingToolTests.cpp:208-263,1246-1287`)으로 다음을 만든다. 접촉 중 측면 버튼 Press/Release(B-06), 근접 이탈과 Space 유지(B-07), 팁 Release 유실 뒤 마우스 입력(B-18), Press → FocusOut → Release 순서 변형(D03), 지우개 ↔ 펜 전환, Release와 LeaveProximity의 순서 교차, `CanvasDisplayWindow`를 거치는 포워딩 경로. 실제 WinTab 장치 확인은 인수인계 1번대로 사람이 한다.

#### T-05 · 중간 · 업데이트 컨트롤러와 시작 경로가 테스트되지 않는다
- **위치**: `UpdateController*`의 직접 테스트 0건. `main.cpp`의 시작 로직은 익명 네임스페이스에 있어 호출할 수 없다.
- **개선**: Velopack/Sparkle 호출을 인터페이스 뒤로 빼서 가짜로 테스트한다(오프라인, busy 중 수동 요청, 시작 대화상자와의 겹침, 확인·다운로드 중 종료, 취소: D18, B-08, U-06). 시작 로직(설정 이전, 락, 인자, 번역기 폴백)은 `app/Startup.cpp`로 분리해 직접 테스트한다.

#### T-06 · 중간 · 포맷 호환 fixture가 빈약하다
- **위치**: 실제 릴리스가 저장한 파일은 `examples/Wave.ugu`(schema 1) 하나와 `tests/fixtures/legacy-render/*.wagle` 3개(schema 9)뿐이다. 구버전 테스트는 대부분 합성 JSON이다(`SerializationBudgetTests.cpp:1256-1284`).
- **영향**: 30개 릴리스에 걸친 사용자 파일 호환성 회귀는 곧 데이터 손실이다.
- **개선**: 주요 릴리스(schema 1~13)마다 대표 파일을 하나씩 저장소에 두고, 열기 → 렌더 digest → 재저장 → 다시 열기를 검증한다. 더 새로운 schema와 알 수 없는 필드의 동작도 고정한다(A-05).

#### T-07 · 낮음 · 테스트 인프라의 잔손질
- 새 테스트를 세 곳(소스 목록 `UguruguSources.cmake:274-323`, 스위트 헤더, 집계 함수)에 등록해야 하는데 누락 검사가 없다.
- CTest 13개 항목이 각자 exe 전체를 환경 변수로 필터해서, 개별 케이스 단위로 재실행하기 어렵다(`UiSelectionTests`는 단일 클래스 3,945줄).
- 고정 대기 9회(`UiSelectionTests.cpp:351,376`, `UiShellTests.cpp:2233,2235`, `UiViewportTests.cpp:1832,1926,2287,2369,2380`), `QThread::msleep(50)`(`UiShellTests.cpp:1400`), 벽시계 임계값(`StrokeCoverageTests.cpp:847,892,974`)이 ASan/Coverage 병렬 실행에서 취약할 수 있다.
- 골든 해시가 Windows와 macOS로 갈라져 있다(`LegacyRenderGoldenTests.cpp:75-100`). 같은 `.ugu`의 내보내기 결과가 OS마다 픽셀 단위로 다르다. 의도된 것이면 문서화하고, 아니면 `src/render`에 `-ffp-contract=off`를 적용하고 수학 함수 의존 부분을 점검한다(원인은 추측).
- **개선**: `tests/*Tests.cpp`와 CMake 목록의 일치를 CI에서 검사한다. 벽시계 임계값은 상대 비교로 바꾼다.

### 9.3 우선적으로 테스트를 붙일 곳 (순서)

1. 클립이 위에 있는 병합의 픽셀 동일성(B-01), 캐시 소유자 예외(B-02). 둘 다 작은 테스트로 높음 등급 결함을 막는다.
2. 저장·복구 실패 주입(T-01).
3. 네이티브 플랫폼 CI 잡과 GPU 표시 테스트(R-04).
4. 합성 태블릿 시나리오(T-04).
5. 승격 프레임 정확성(T-03).
6. 포맷 호환 fixture(T-06).
7. 업데이트와 시작 경로(T-05).
8. 패키지 smoke 확장: 형식별 디코드, 번역 로드, 라이선스 전체(R-01, U-01, L-03).

---

## 10. 잘 된 부분 — 유지할 설계

리팩터링하면서 실수로 바꾸지 않아야 할 것들이다.

1. **트랜잭션 커밋**: 후보 복사본 → `prepare`(검증과 바이트 계획) → `DocumentDelta` → push 순서다. prepare가 실패하면 상태가 바뀌지 않는다(`DocumentController.cpp:1190-1306`). undo/redo는 목표 상태를 먼저 만들고 `compactSize`로 교차 검증한 뒤에만 커서를 움직인다(`:220-262`, `DocumentUndoStack.cpp:161-207`).
2. **콘텐츠 리비전과 히스토리 노드 분리**: 병합으로 변경이 상쇄되면 리비전이 원래대로 돌아간다(`DocumentController.cpp:1668-1679`). 백그라운드 저장 완료 판정에 이것을 쓴다(`MainWindow.cpp:1042-1051`).
3. **delta 히스토리와 COW 공유, 백킹 주소 기반 메모리 회계**(`HistoryMemory.hpp:22-76`), 저장용 `frozenCopy`(`HistoryEffects.hpp:120-125`).
4. **엔진의 UI 독립성**: `ugurugu_core`가 Widgets를 링크하지 않는다(`UguruguTargets.cmake:11-24`).
5. **불변 스냅샷 기반 스레딩**: 워커는 `shared_ptr<const Document>`와 취소 토큰만 받고 `this`를 캡처하지 않는다. generation으로 낡은 결과를 버린다(`CanvasWidgetPreview.cpp:926-995,1432-1442,1571-1578`). 모든 람다 `connect`에 컨텍스트 객체가 있다.
6. **결정적 우글거림**: 전역 RNG 없이 `(seed, frame, index, channel)` 해시만 쓴다(`DeterministicNoise.cpp:25-54`). 획마다 seed를 저장한다. Classic의 v1 호환 불변량을 상수와 주석으로 고정했다.
7. **`StaticLayerCache`의 정확성 설계**: 키와 별개로 implicit sharing 포인터로 소스 일치를 확인하므로 편집 뒤 낡은 래스터를 돌려줄 수 없다(`StaticLayerCache.cpp:63-68`). 렌더는 락 밖에서 하고, 두 mutex를 중첩해서 잡지 않는다. 전체 레이어 렌더가 반드시 거치는 단일 관문은 `renderPaintLayerImage`(`LayerHierarchyCompositor.cpp:47-114`)다. B-02는 이 설계에 RAII만 더하면 된다.
8. **증분 렌더의 "전체 렌더와 픽셀 동일" 원칙**과 체크포인트, 16.16 고정소수점 샘플링(`ImageResampler.cpp:13-31`), 불확실하면 전체 렌더로 돌아가는 폴백(`RenderEngineStrokes.cpp:293-296`).
9. **입력 경계 방어**: bounded inflate(헤더, 정확한 출력 길이, 전체 입력 소비, `Z_STREAM_END` 확인, `BoundedCompression.cpp:14-64`). 로드할 때 콘텐츠 해시 id를 재검증한다. 정확한 바이트 계획과 최종 일치를 검사한다(`PreparedPlanBuilder.hpp:617-620`). seed와 revision을 10진 문자열로 저장해 정밀도 손실을 막는다. Wawa 리더의 경계 검사 커서와 후행 데이터 거절도 있다.
10. **원자적 저장**: 모든 쓰기에 `QSaveFile`을 쓰고 취소 시 임시 파일을 정리한다. `RecoveryWriter`의 generation 가드와 소멸 시 flush, 읽을 수 없는 복구본은 지우지 않고 quarantine한다.
11. **최근 지연 개선 구조**: `CanvasDisplayWindow`(자체 스왑체인, 입력 투명, 포워딩), `cacheKey` 기반 dirty 영역 부분 업로드, overlay 분리, 합성 프리뷰 버퍼의 제자리 패치(`c9d6808`). 측정으로 확인된 설계다.
12. **입력 처리의 세부**: `TabletRelease` 압력 0을 마지막 샘플로 대체(`CanvasWidgetTools.cpp:170-175`), 지우개 끝 감지와 커서 반영, 펜이 터치를 억제하는 우선순위, 이벤트 타임스탬프 기반 1€ 안정화.
13. **ShortcutBinding**: 사용자 지정, 기본값, 별칭을 분리하고 중복을 검사한다(`ShortcutBinding.cpp:74-176`, `SettingsDialog.cpp:370-397`). 이것을 토대로 U-02와 U-12를 고치면 된다.
14. **배포 안전장치**: 의존성과 액션의 SHA 고정, 릴리스 3중 게이트, macOS 서명·공증·감사 파이프라인, Windows 격리 smoke와 음성 대조군(`TestWindowsPackage.ps1:44-92,308-327`), `qt.conf` 플러그인 고정.
15. **문서화 규율**: "왜"를 적는 주석, project-status의 ID·증거·측정 조건 관리, 3개 언어 릴리스 노트 강제.

---

## 11. 우선순위별 개선 로드맵

각 단계는 project-status의 공통 완료 규칙(수정 전 실패하고 수정 후 통과하는 회귀 테스트, 실행 환경 기록, 성능은 같은 조건 A/B)을 따르는 것을 전제로 한다.

### 바로 고칠 것 (각각 수 시간~1일, 변경이 작고 위험이 낮음)

| 순서 | 항목 | 내용 |
|---|---|---|
| 1 | B-01 | 병합 상태 검사에 "바로 위 형제가 클립인가"를 추가하고, 픽셀 비교 테스트를 붙인다 |
| 2 | B-02 | 두 캐시의 소유자에 RAII 가드를 두고, 소유자 예외 테스트를 붙인다 |
| 3 | B-03 | 그림자 bake를 뷰포트와 교차한 영역으로 제한하고, 크기 상한 테스트를 붙인다 |
| 4 | U-01 | `qt` → `qtbase` 순으로 번역기를 로드하고, 패키지 smoke에서 번역 로드를 확인한다 |
| 5 | R-01 | `qtimageformats` 모듈을 추가하고 macOS 플러그인 목록을 넓힌다(또는 필터를 축소). smoke에서 형식별 디코드를 확인한다 |
| 6 | U-02 | Redo와 표준 키의 나머지 바인딩을 별칭으로 등록한다 |
| 7 | B-15 | 바인딩 객체를 재사용한다(dangling 제거) |
| 8 | P-05 | 계층 분석을 한 번만 해서 공유한다 |
| 9 | Q-03 일부 | `StrokePresence`와 `duplicateStrokes`의 도달 불가 분기를 제거한다(P-04 일부 즉효) |
| 10 | B-08 | 업데이트 작업을 전용 풀로 옮기고, 종료 시 락을 join보다 먼저 해제한다 |
| 11 | L-01, L-03 | NOTICES에 Qt 버전·모듈과 Mesa·D3Dcompiler를 추가하고, smoke 라이선스 목록을 보완한다 |
| 12 | B-06, B-07 | 접촉 중 측면 버튼 무시, 근접 이탈에서 수정자 유지. 합성 이벤트 테스트를 붙이고, 인수인계 1번(실제 펜 확인) 때 함께 검증한다 |

### 단기 (1~4주)

- **정확성**: B-04(split 합성 정확화 또는 반투명 시 승격 금지, T-03), B-05(`documentForLayer` 강제), B-19(클립 기준 상실 정책).
- **실패 경로**: B-09(열기 사전 검사와 `bad_alloc` 처리), B-10(`HistoryMacroScope` RAII), B-11(`qFatal` 제거, 엔진 로그 카테고리), B-12(저장 완료 가드), B-13(종료 시 자동저장 정지, 보존 파일 상한), B-14(선택 항목을 개수 한도에서 제외). T-01, T-02를 같이 진행한다.
- **표시와 입력**: B-16(device lost 재초기화), B-18, Q-04(`Interaction` enum. A-03의 4단계를 앞당김).
- **재생과 갱신 비용**: P-01, P-02, P-03, P-07, P-08, P-10(문서 교체와 내보내기 시 캐시 정리). 각 항목은 측정 → 수정 → 재측정 순서로 한다.
- **CI와 배포 검증**: R-04(네이티브 플랫폼 잡), R-05(서명 앱 기동), R-08(퍼저 시드와 corpus), R-09(`RELEASING.md`, 키 백업).
- **UX**: U-03(파일 연결과 단일 인스턴스 전달, D09), U-04, U-05, U-06(업데이트 옵트아웃과 비모달), U-09(로그 폴더 열기, 장치 상태), U-10, U-11(첫 화면 배치, README·도움말 정합).
- **라이선스**: L-02(폰트 고지).
- **테스트**: T-04, T-05, T-06.

### 장기 (1~3개월 이상)

- **구조**:
  - A-03: CanvasWidget을 ToolReference → Text → Viewport → Interaction → DisplaySource → PreviewPipeline 순으로 분리한다.
  - A-02와 Q-02: 편집 빌더, `commit()` 결과 계약(D07), HistoryTransactionManager를 도입한다.
  - A-01: 검증과 `PreparedDocument`를 `document/`로 옮기고 데스크톱에서도 엔진 라이브러리를 분리한다.
  - A-04: DocumentSession과 ActionRegistry를 도입한다.
  - Q-01: 재생 루프를 하나로 합친다.
- **포맷**: A-05 명세 문서, 마이그레이션 단계 함수, 새 schema 안내, 레거시 확장자 처리.
- **성능(측정 우선)**: P-04(20,000획 규모의 pen-up과 undo), P-06(프레임 무관 기하, `maskPath`, Fill, Image 캐시), P-09(취소 단위), P-17(뷰포트 고해상 모드), 인수인계 5번(PresentMon으로 표시 지연을 측정한 뒤 렌더 스레드나 latency waitable 판단).
- **배포 보안**: R-02(Windows 서명), R-03(서명 잡 분리, environment 게이트), R-06(Windows tidy·ASan), R-07(의존성 스캔, SBOM, 버전 단일 소스), R-17(서명 태그, provenance).
- **접근성과 국제화**: U-07(레이어 메뉴, 키보드 경로, 색 기록 roving), U-08(고대비·시스템 색 구성표, 라이트 테마), U-13(macOS 관례), U-14(오류 문구 번역), 압력 곡선.
- **결정성**: 플랫폼 간 렌더 결정성 조사(T-07, `-ffp-contract`).
- **기존 인수인계 유지**: 실제 펜 확인, UI 육안 확인, 첫 이동 시 2회 paint, macOS Metal 실행 검증(`docs/project-status.md` §5 "인수인계").

---

## 부록

### A. 기존 project-status ID와의 대응

| 기존 ID | 이 보고서 | 추가된 내용 |
|---|---|---|
| D01, D20 | Q-03 | 마스크 없는 변환 경로는 데스크톱 앱에서 호출자가 없다. 떠 있는 선택 세션 부분만 프로덕션 경로다 |
| D02 | B-04 | 승격 프레임이 split 합성일 때 ±2 불일치 가능성(반투명 위쪽 레이어) |
| D03 | B-06, B-07, B-18, Q-04, U-06 | 측면 버튼, 수정자 초기화, 시퀀스 고착, 업데이트 모달에 의한 획 취소 |
| D04 | B-09, R-08 | DOM 증폭, 열기 경로의 `bad_alloc`, 퍼저 시드가 디코더에 닿지 않음 |
| D05 | U-19, R-04 | 필터와 접미사 UX, 네이티브 대화상자 CI 공백 |
| D07 | Q-02, B-11, A-02 | 실패 배관 호출 수, 누락 위치, 오해를 부르는 메시지, `qFatal` |
| D08 | P-01, P-07, P-08, P-10 | 캐시 미정리, 3중 직렬화 캐시, 표시 표면, warmup 폭풍, 우선순위 |
| D09 | U-03 | plist 문서 타입 부재, Windows 연결 부재, 두 번째 인스턴스 처리 |
| D10 | R-01 | 원인 확정(모듈 미설치, macOS 목록), 설치 트리 확인 |
| D15 | U-01, U-14 | 배포본 Qt 표준 문자열 미번역(원인과 수정안) |
| D16 | R-05, R-07 | 서명 앱 미기동, 의존성 추적 자동화 |
| D17 | U-02, U-11, U-12, U-13 | Redo 별칭, macOS 수정자 충돌, 하드코딩 목록 |
| D18 | B-08, U-06, T-05 | 종료 지연, 인스턴스 락, 옵트아웃 |
| D19 | B-13 | 보존 파일 누적, 위치 미안내, 시작 실패 시 종료 |
| D21 | P-02, P-14, P-15 | 개미 타이머, 전역 필터, 문서 복사 |
| D22 | A-01~A-04 | 분리 순서와 인터페이스 구체화 |
| D23 | Q-01, Q-03 | 계약에 영향을 주는 중복, 죽은 코드 목록 |
| D24 | R-04, R-06, R-08, T-02, T-07 | GPU 테스트 SKIP 위치, Windows 정적 분석 공백 |
| D25 | R-09, U-11 | 릴리스 런북, README 불일치 목록 |
| R12, R13, R14 | U-05, U-07, U-08, U-11 | 스와치 교환 결함, 대비 수치, 핵심 패널 숨김 |

### B. 서브 에이전트 분석 범위 (모두 Sonnet 5.5, 읽기 전용)

1. 문서 모델·히스토리·코어 경계: `src/document/**`, `src/brush`, `src/input`, `MemoryBudget`
2. 렌더 엔진·우글거림: `src/render/**`, 셰이더
3. IO·저장·앱 서비스·시작: `src/io/**`, `src/app/**`, `main.cpp`, 저장·복구 관련 UI
4. 캔버스·입력·표시: `CanvasWidget*`, `CanvasDisplayWindow`, `CanvasViewport`, 재생 UI
5. 메인 윈도우·UX·i18n·접근성: `MainWindow*`, 도크, 팝오버, 대화상자, Theme, `.ts`, README
6. 빌드·CI·배포·라이선스·문서·테스트: `cmake/`, `.github/`, 리소스, 문서, `tests/**`, `tools/*.cpp`

### C. 직접 재확인한 항목

빌드 로그와 경고, CTest와 QTest 집계, 설치 트리(번역, 이미지 플러그인, 동봉 DLL, 라이선스 파일)는 직접 실행해서 확인했다. 코드로 직접 다시 확인한 지적은 B-01, B-02, B-03, B-04(합성 순서, 테스트 허용 오차, 승격 캐시), B-06(코드 경로), B-08, B-11(`qFatal`), B-14, P-01(코드 경로), Q-03(호출자 grep), U-01, U-02, R-01, R-04, L-01이다.
