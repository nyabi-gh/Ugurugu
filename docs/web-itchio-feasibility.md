# Ugurugu 웹·itch.io HTML5 이식 타당성 보고서

> 상태 정리(2026-09-08): **조사 완료 자료**다. 아래의 "현재", 미구현 설명과
> 일정 추정은 조사 당시 기준이며, 이후 엔진·웹 셸·스테이징 구현을 반영한
> 작업 현황은 [웹 이식 진행 현황](web-port-progress.md)을 따른다.
> 전체 단계 0~6의 구현·검증이 완료됐다는 의미는 아니다.

- 조사 기준일: 2026-08-06
- 조사 대상 커밋: `a6ac90bb6377f1dec401db480ceb44a4708f6b79`
- 조사 범위: 저장소 정적 분석, itch.io·Qt·Emscripten·브라우저 문서 조사
- 제외한 작업: 코드 수정, 리팩터링, 빌드, WebAssembly PoC, itch.io 시험 업로드

## 1. 결론

**itch.io에 웹 버전을 배포하는 것은 가능하다.** 다만 현재 macOS/Windows용 Qt Widgets 실행 파일을 그대로 올릴 수 있는 것은 아니며, 별도의 WebAssembly 빌드 타깃과 웹 UI·저장소·입력 계층이 필요하다. 현재 코드에서 가장 재사용 가치가 큰 부분은 CPU 기반 문서/렌더링/직렬화 코어이고, 가장 큰 이식 장애물은 itch.io의 ZIP 규격이 아니라 데스크톱 전용 UI와 동기식 파일·대화상자·스레드·메모리 모델이다.

네 가지 방향 중 **3번, “핵심 C++ 로직만 WebAssembly로 재사용하고 UI는 TypeScript로 새로 구현”**을 권장한다. 단, “C++ 코어”를 Qt와 완전히 분리하는 방식은 아니다. `QImage`, `QPainter`, `QJsonDocument`, `QTransform` 등에 대한 의존이 깊으므로, **Qt 6.11.1 Core/Gui를 포함한 단일 스레드 Wasm 엔진**으로 시작하고 Widgets/Concurrent/GuiPrivate는 웹 빌드에서 제외하는 구성이 현실적이다.

| 판단 항목 | 결론 |
|---|---|
| 데스크톱 브라우저 MVP | 실현 가능성이 높음 |
| 현재 Qt Widgets 앱 전체의 즉시 변환 | 컴파일 가능성은 있으나 현재 구조 그대로는 불가 |
| 모바일에서 데스크톱 기능 동등성 | 단기적으로 비현실적 |
| itch.io HTML5 적합성 | 3번 방향은 높음, 전체 Qt 이식은 중간 |
| 권장 기준 그래픽 API | WebGL 2, Canvas 2D 표시 폴백 |
| WebGPU | 장기 선택 기능, MVP 기준 기능으로 사용하지 않음 |
| 멀티스레딩 | MVP 목표는 Wasm Pthreads 없이 Dedicated Worker 1개. Qt engine의 Worker 구동은 선행 gate |
| 파일 호환 | 기존 `.ugu` 바이트 직렬화 경로를 이용해 유지 가능 |
| 대략적 작업량 | 조사·spike 포함 MVP 8.25~13.0 인월, 폭넓은 동등성까지 총 14.25~23.0 인월 |

이 결론의 핵심 근거는 다음과 같다.

1. 빌드가 이미 `ugurugu_core`와 `ugurugu_ui`로 나뉘며, 코어는 의도적으로 Qt Widgets에 연결되지 않는다(`cmake/UguruguTargets.cmake:9-39`).
2. 실제 그림 생성은 GPU 셰이더가 아니라 `QImage`/`QPainter` 기반 CPU 렌더러가 담당한다(`src/render/RenderEngine.cpp:127-249,487-508`, `src/render/engine/LayerHierarchyCompositor.cpp:45-145`, `src/render/StrokeRenderer.cpp:789-869`). 따라서 WebGPU로 전체 렌더러를 다시 만들 필요가 없다.
3. 프로젝트 직렬화기에 파일 경로 API와 별도로 `QByteArray` 입출력 API가 이미 있다(`src/io/DocumentSerializer.hpp:162-182`). 브라우저 `File`/`Blob`과 연결하기 좋은 경계다.
4. 반대로 UI는 `QMainWindow`, 도킹 패널, 동기식 모달 대화상자와 파일 경로를 중심으로 설계되었다. 메인 창 최소 크기도 900×640이고 캔버스 입력은 QWidget의 마우스·펜·터치 이벤트 상태기에 결합되어 있다(`src/ui/MainWindow.cpp:163-190`, `src/ui/CanvasWidget.cpp:51-60`, `src/ui/CanvasWidgetEvents.cpp`).
5. 현재 메모리 정책은 4 GiB 프로세스 예산과 최대 2 GiB 미리보기 캐시를 전제로 한다(`src/app/MemoryBudget.hpp:15-39`). 브라우저, 특히 모바일 탭에 그대로 적용할 수 없다.

## 2. 조사 방법과 신뢰 범위

저장소에서는 CMake 타깃·소스 목록, `src`의 UI/문서/렌더링/입출력/앱 서비스, 테스트 구성을 추적했다. 추적된 C++/Objective-C++ 물리 행 수를 기준으로 제품 소스는 약 52.5k행, 테스트는 약 25.9k행이다. UI 계층은 약 22.9k행이고, 문서·입력·브러시·렌더·직렬화 중심의 재사용 후보는 약 25.6k행, 즉 제품 소스의 약 48.8%다. 이 비율은 “바로 컴파일되는 코드 비율”이 아니라 방향별 재사용 규모를 비교하기 위한 상한 지표다.

외부 요구사항은 2026-08-06에 다시 확인했다. 출처 상태는 다음처럼 구분한다.

- **현재 공식**: itch.io Creator/Butler 문서, Qt 6.11 문서처럼 현재 제품 문서에 게시된 내용
- **현재 기술 기준**: Emscripten 및 MDN/WebKit의 현재 문서
- **실험적 운영 공지**: itch.io 관리자가 게시했으나 “Experimental”로 명시된 SharedArrayBuffer 지원
- **과거/참고**: 현재 공식 문서가 아니거나 최신 내용에 의해 대체된 포럼·구버전 정보

브라우저 메모리, 저장 용량, 펜 압력 품질처럼 고정된 보장값이 없는 항목에는 특정 수치를 사실처럼 부여하지 않았다. 아래의 웹용 1024/2048 픽셀 제한과 캐시 수치는 구현 목표이며, 브라우저 표준의 보장값이 아니다.

## 3. 현재 코드베이스

### 3.1 UI 프레임워크와 애플리케이션 구조

애플리케이션은 Qt 6 Widgets 기반이다.

- `src/main.cpp:169-310`에서 `QApplication`, 번역기, 단일 인스턴스 잠금, 메인 창, 이벤트 루프, 종료 시 백그라운드 작업 정리를 구성한다.
- `src/ui/MainWindow.hpp:39-61`의 메인 창은 `QMainWindow`다.
- `src/ui/MainWindow.cpp:163-266`은 중앙의 `CanvasWidget`/`TimelineBar`와 Tool, Color, Color History, Wobble, Layer 다섯 도크, 중첩/탭 도킹, 창 위치·상태 복원을 구성한다.
- 창 최소 크기는 900×640, 초기 크기는 1280×820이다(`src/ui/MainWindow.cpp:168,230-243`). 작은 화면에 맞춘 반응형 레이아웃이 아니다.
- 정적 검색 기준 UI에는 모달 `exec()` 호출 지점이 10개이고, `QFileDialog::get*`/`QColorDialog::getColor` 같은 동기식 선택 API가 13개 있다. 대표 위치는 `src/ui/MainWindow.cpp:880-960`, `src/ui/MainWindowExport.cpp:63-152`, `src/ui/MainWindowSettings.cpp:356-437`이다.

Qt for WebAssembly는 Widgets 모듈 자체를 지원하지만, **기본 Qt Wasm build**는 브라우저 메인 스레드에서 중첩 이벤트 루프를 만들 수 없으므로 `QDialog::exec()`를 지원하지 않는다. Asyncify로 우회할 수 있으나 bundle·CPU overhead가 생기고, JSPI는 Qt 소스 build와 브라우저 지원 검증이 필요하다. 현재처럼 모달 흐름이 넓은 앱의 공개 baseline으로 두기보다 비동기 UI로 바꾸는 편이 안전하다. 파일 선택도 `getOpenFileContent()`와 `saveFileContent()` 같은 비동기 웹 전용 흐름으로 바꿔야 한다. 또한 웹에서 `QApplication::exec()`는 호출자에게 돌아오지 않으므로, 현재 `application.exec()` 뒤의 로그와 `joinDetachedBackgroundWork()`, `Logging::shutdown()`(`src/main.cpp:296-309`)에 종료 의미를 의존할 수 없다. Qt 공식 문서의 해당 제약과 우회책은 [Q1](https://doc.qt.io/qt-6/wasm.html), [E7](https://emscripten.org/docs/porting/asyncify.html)에 명시되어 있다.

따라서 “Qt Widgets가 지원된다”와 “현재 UI가 수정 없이 동작한다”는 다른 판단이다. 1번 방향도 상당한 비동기 상태 전환과 UI 재배치를 요구한다.

### 3.2 입력: 마우스·펜·터치·키보드

현재 캔버스 입력은 데스크톱 중심이다.

- `src/ui/CanvasWidget.cpp:51-60`은 마우스·태블릿 추적과 `Qt::WA_AcceptTouchEvents`를 켠다.
- `src/ui/CanvasWidgetEvents.cpp`는 마우스와 휠, 태블릿 압력과 지우개 포인터, 두 손가락 이동·확대·회전을 처리한다. 마우스 압력은 1.0으로 취급한다.
- `src/ui/CanvasWidgetEvents.cpp:882-936`은 Space, Escape, Delete/Backspace를 직접 처리한다.
- `src/ui/MainWindowActions.cpp:74-129,680-691` 및 `src/ui/ShortcutBinding.cpp`에는 새 문서, 열기, 저장, 종료, 실행 취소, 잘라내기/복사/붙여넣기 등에 대한 Ctrl/Cmd 단축키가 있다.

웹에서는 이 계층을 Pointer Events로 다시 작성해야 한다. `pointerType`, `pressure`, tilt를 읽고, 드로잉 중 `setPointerCapture()`를 사용하며, 캔버스에만 `touch-action: none`을 적용해야 한다. 그렇지 않으면 모바일 브라우저의 스크롤/확대가 포인터 시퀀스를 `pointercancel`로 끝낼 수 있다([B3](https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events), [B4](https://developer.mozilla.org/en-US/docs/Web/CSS/touch-action)).

iframe 안에서 입력 API가 사라지는 것은 아니지만 다음 제약이 있다.

- 시작 클릭 후 캔버스가 명시적으로 포커스를 받아야 키보드 이벤트를 안정적으로 받는다.
- 브라우저/운영체제가 예약한 Cmd/Ctrl+W, Cmd/Ctrl+L, 일부 확대·새로고침 단축키는 앱이 소유한다고 가정할 수 없다.
- 펜 압력·tilt·지우개 식별은 브라우저, 운영체제, 장치 드라이버 조합에 따라 달라진다. itch.io는 펜 품질을 별도로 보장하지 않는다.
- 전체 화면 요청은 사용자 활성화와 iframe의 Fullscreen 권한 정책이 필요하다([B6](https://developer.mozilla.org/en-US/docs/Web/API/Element/requestFullscreen)). itch.io가 제공하는 시작/전체 화면 버튼을 기본 경로로 사용하고, 앱 자체 요청은 보조 기능으로 취급해야 한다.

### 3.3 렌더링 방식

현재 렌더링은 **CPU 래스터 엔진 + GPU 표시 계층**이다.

- `src/render/RenderEngine.cpp:127-249,487-508`과 `src/render/engine/LayerHierarchyCompositor.cpp:45-145`은 `QImage` surface를 만들고 `QPainter`로 layer/hierarchy를 합성한다.
- `src/render/StrokeRenderer.cpp:789-869`은 선, 에어브러시, 스프레이를 CPU로 래스터화한다.
- `src/render/ImageResampler.cpp:13-31,132-191`은 결정적 고정소수점 리샘플링을 구현한다.
- `src/render/FloodFillMask.cpp:12-98`은 CPU scanline flood fill이다.
- `src/ui/CanvasFrameView.hpp:17-23`의 설명대로 GPU는 CPU가 합성한 프레임을 texture quad로 표시하고 dirty region만 다시 올린다.
- `src/ui/CanvasFrameView.cpp:153-268`은 BGRA `QImage`를 `QRhiTexture`에 업로드한다.
- `src/ui/CanvasWidgetEvents.cpp:77-112`에는 `QPainter` 소프트웨어 표시 폴백도 있다.

이 구조는 웹 이식에 유리한 면이 있다. 그림 결과를 결정하는 코드가 QRhi나 플랫폼 GPU API에 묶이지 않았으므로 `QImage`/`QPainter`가 Wasm에서 정확히 동작하면 기존 픽셀 결과를 재사용할 수 있다. 반면 프레젠테이션 계층인 `CanvasFrameView`는 `QRhiWidget`, `rhi/qrhi.h`, `Qt6::GuiPrivate`에 의존한다(`src/ui/CanvasFrameView.hpp:6-10`, `cmake/UguruguTargets.cmake:26-46`). QRhi 계열은 Qt 버전 간 소스·바이너리 호환을 보장하지 않고 웹 지원이 별도로 약속되지 않았으므로 [Q3](https://doc.qt.io/qt-6/qrhiwidget.html), 웹에서는 교체 대상으로 보는 편이 안전하다.

권장 방향에서는 C++ CPU 렌더 결과를 WebGL 2 texture로 표시한다. Canvas 2D는 프레젠테이션 계층의 폴백일 뿐 “WebGL이 전혀 없는 환경”까지 지원한다는 의미는 아니다. Qt 공식 문서는 애플리케이션이 직접 GPU를 쓰지 않아도 Qt Wasm 자체에 고정 WebGL 요구가 있다고 설명하므로, headless Core/Gui worker build가 이 요구를 줄일 수 있는지는 PoC에서 확인해야 한다. WebGPU로 렌더러를 재작성할 이유가 없으며, WebGPU는 현재도 MDN에서 “Limited availability”로 분류된다([Q1](https://doc.qt.io/qt-6/wasm.html), [B16](https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API)).

### 3.4 문서 모델과 파일 포맷

문서 모델은 기능적으로 재사용 가치가 높지만 Qt 자료형과 깊게 결합되어 있다.

- `src/document/Document.hpp:6-16`은 `QByteArray`, `QColor`, `QImage`, `QMap`, `QPointF`, `QTransform`, `QUuid`, `QVector`를 사용한다.
- `src/document/Document.hpp:23-263`은 paint/erase/fill/image 작업, 선택 마스크, 레이어/그룹, 합성 경계, 브러시 엔진, wobble/animation 상태를 담는다.
- `src/document/DocumentController.hpp:91-180`은 `QObject` 기반 변경 신호, 히스토리, 저장 상태를 관리한다.
- `src/io/serializer/SerializerSchema.hpp:19-28`은 현재 스키마 버전 13, 알고리즘 버전 3을 정의한다.
- `src/io/DocumentSerializer.cpp:565-651`은 경로 기반 `QSaveFile`/`QFile` 외피를 사용하지만, `src/io/DocumentSerializer.hpp:171-182`은 `toJson()/fromJson(QByteArray)`를 제공한다.
- `src/io/serializer/RasterAssetTable.cpp`과 `MaskAssetTable.cpp`은 raster/mask payload 압축, 식별과 크기 제한을 담당한다.
- `src/io/WawaV10Reader.hpp:82-94`, `WawaV10Importer.hpp:34-41`도 바이트 입력 API가 있어 후속 단계에서 재사용 가능하다.

`.ugu`는 ZIP 컨테이너가 아니라 compact JSON과 압축·base64 payload를 사용하는 스키마 버전 파일이다. 브라우저에서는 경로 기반 함수를 호출하지 않고 입력 `ArrayBuffer`를 `QByteArray`로 넘겨 `fromJson()`을 호출하며, 저장 시 `toJson()` 결과를 `Blob` 다운로드 또는 IndexedDB에 기록하면 된다. 이 경계가 데스크톱과 웹 파일 호환성을 유지하는 가장 강한 자산이다.

Qt를 제거한 순수 Emscripten 이식을 시도하면 위 자료형과 `QPainter` 동작을 모두 대체해야 한다. 이는 “기존 C++ 직접 컴파일”이라기보다 문서 모델과 렌더러의 재작성에 가까워진다.

### 3.5 파일 입출력·복구·내보내기·클립보드

현재 파일 경로와 프로세스 영속성에 결합된 부분은 웹 어댑터로 바꿔야 한다.

- 문서 열기/이미지 가져오기는 동기식 `QFileDialog`와 `QImageReader`를 사용한다(`src/ui/MainWindow.cpp:1451-1517`).
- 저장 및 “변경 사항 저장?” 흐름은 모달 대화상자와 경로를 사용한다(`src/ui/MainWindow.cpp:880-960`).
- 프리셋 가져오기/내보내기는 `QFile`/`QSaveFile`을 사용한다(`src/ui/MainWindowSettings.cpp:356-437`).
- 자동 저장은 30초 타이머다(`src/ui/MainWindow.cpp:223-226`). `src/app/RecoveryStore.cpp:189-235,288-386`은 `QStandardPaths`, `QFile`, `QSaveFile`, filesystem rename/quarantine에 의존한다.
- `src/app/ApplicationInstanceLock.cpp:17-83`은 앱 로컬 경로의 `QLockFile`을 사용하고, `src/app/Logging.cpp:53-100`은 회전 파일 로그를 쓴다.
- `src/ui/MainWindowExport.cpp:33-152`의 이미지/GIF/WebP 내보내기는 모두 파일 경로와 백그라운드 작업자에 연결되어 있다.
- `src/io/ExportWorker.cpp:308-375`은 애니메이션 프레임 전체를 `QVector<QImage>`에 보유한 뒤 GIF/WebP encoder로 넘긴다.

클립보드는 `src/io/SelectionClipboardCodec.hpp:21-24`의 단일 레이어 프로젝트 JSON을 custom MIME으로 기록하고, `src/ui/CanvasWidget.cpp:569-593`에서 custom MIME과 raster를 함께 설정한다. `src/ui/MainWindow.cpp:1113-1177`은 custom MIME만 내부 붙여넣기로 받고 외부 이미지 붙여넣기는 지원하지 않는다.

웹용 대응은 다음과 같다.

- 열기: `<input type="file">`/File API를 기준 경로로 사용한다.
- 저장: `Blob`과 `download` 링크를 기준 경로로 사용한다. File System Access API는 지원 브라우저에서만 “같은 파일에 다시 저장” 기능으로 추가한다.
- 자동 복구: JS 계층의 IndexedDB에 바이트 snapshot과 metadata를 비동기로 기록한다. IDBFS에 데스크톱 경로를 흉내 내기보다 저장소 비동기성을 API에 명시한다.
- 클립보드: 앱 내부 복사 버퍼를 항상 제공하고, 표준 이미지/텍스트의 OS 클립보드 복사는 사용자 동작 안에서만 시도한다. custom `application/octet-stream` 또는 itch.io iframe의 clipboard 권한 위임을 필수 기능으로 두지 않는다.
- 내보내기: MVP는 현재 프레임 PNG 바이트만 제공한다. 전체 프레임을 보유하는 GIF/WebP 내보내기는 메모리 구조를 streaming 방식으로 바꾼 뒤 추가한다.

브라우저 파일 시스템과 클립보드 제약은 [Q1](https://doc.qt.io/qt-6/wasm.html), [B7](https://developer.mozilla.org/en-US/docs/Web/API/File_API), [B8](https://developer.mozilla.org/en-US/docs/Web/API/Window/showOpenFilePicker), [B9](https://developer.mozilla.org/en-US/docs/Web/API/HTMLAnchorElement/download), [B10](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API)에 근거한다.

### 3.6 멀티스레딩

현재 앱은 스레드를 선택적 최적화가 아니라 여러 객체의 수명 구조에 포함한다.

- `ExportWorker` 생성자는 항상 `QThread`를 시작하고, 소멸자는 취소 후 무기한 `wait()`한다(`src/io/ExportWorker.cpp:49-65`).
- `RecoveryWriter`도 생성 시 스레드를 시작하고 소멸 시 flush 후 `wait()`한다(`src/app/RecoveryWriter.cpp:19-40`).
- 두 객체는 `MainWindow`의 값 멤버다(`src/ui/MainWindow.hpp:176-188`).
- 캔버스 미리보기는 1~8 스레드의 전용 `QThreadPool`을 만든다(`src/ui/CanvasWidget.cpp:172-177`).
- 프레임 캐시, 선택 계산, 레이어 thumbnail은 각각 `QtConcurrent::run`을 사용한다(`src/ui/CanvasWidgetPreview.cpp:842-975`, `CanvasWidgetSelection.cpp:355-433`, `LayerDock.cpp:904-1016`).
- 종료 시 전역 풀 완료를 기다린다(`src/app/BackgroundWork.cpp:11-14`, `src/main.cpp:308`).

Qt/Emscripten Pthreads는 Web Worker 위에 구현되지만 `SharedArrayBuffer`와 COOP/COEP로 만든 cross-origin isolation이 필요하다. 메인 브라우저 스레드의 `pthread_join`/조건 대기는 멈춤이나 교착을 일으킬 수 있고, threaded/non-threaded를 하나의 Wasm 바이너리로 자동 폴백할 수도 없다([E1](https://emscripten.org/docs/porting/pthreads.html)).

itch.io의 SharedArrayBuffer 기능은 2026-08-06에도 “Experimental” opt-in이다([I3](https://itch.io/t/2025776/experimental-sharedarraybuffer-support)). Pthreads를 켜지 않은 일반 Worker는 공유 메모리를 사용하지 않으므로 COOP/COEP가 필요 없다. 다만 **Qt Core/Gui Wasm 엔진 전체를 Dedicated Worker에서 초기화하는 조합은 Qt의 문서화된 지원 조합이 아니며, 이 보고서의 가장 중요한 미확인 가설**이다. 따라서 다음 항목은 단계 1 gate를 통과할 때의 MVP 목표다.

- Wasm 내부는 단일 스레드
- 기술 spike가 성공하면 Wasm 엔진 전체를 Dedicated Worker 한 개가 소유
- UI와 엔진은 메시지와 transferable buffer로 통신
- `ExportWorker`/`RecoveryWriter`/`QtConcurrent` 경로는 웹 빌드에서 사용하지 않음
- 멀티스레드 Wasm은 성능 수치가 필요성을 입증하고 itch.io staging에서 브라우저 행렬을 통과한 뒤에만 별도 artifact로 검토

### 3.7 메모리와 성능

현재 문서 제한과 데스크톱 메모리 예산은 웹에 비해 크다.

- `src/document/DocumentLimits.hpp:13-53`: 최대 캔버스 4096, 프레임 60, 레이어 256, 프로젝트 128 MiB, distinct decoded raster 256 MiB, mask 256 MiB
- `src/app/MemoryBudget.hpp:17-39`: resident 4 GiB, history 192 MiB, serialization 작업 512 MiB, animation export 512 MiB, preview cache 128 MiB~2 GiB
- `src/app/MemoryBudget.cpp:30-71`: 설치 메모리는 Windows/macOS에서만 읽고 그 외 플랫폼은 0을 반환하므로 웹에서는 최소 preview cache 128 MiB가 선택될 가능성이 높음
- `src/io/AnimationExportPolicy.cpp:12-36`: 애니메이션 작업량을 pixel×frame×12 bytes로 추정

4096×4096 ARGB 한 프레임은 약 64 MiB이고 60장을 단순 보유하면 약 3.75 GiB다. 현재 `ExportWorker`의 전체 프레임 보유 방식까지 포함하면 모바일 브라우저에서는 현실적인 상한이 아니다. Wasm32의 주소 공간 이론값과 실제 탭이 확보할 수 있는 연속 메모리는 다르며, 브라우저는 장치 상황에 따라 탭을 종료할 수 있다([B17](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Memory/Memory)). Emscripten의 memory growth도 할당 성공을 보장하지 않고 성장 시 heap view 교체와 지연을 발생시킬 수 있다([E4](https://emscripten.org/docs/tools_reference/settings_reference.html)).

Qt 6.11 문서의 `QT_WASM_INITIAL_MEMORY` 기본값은 50 MB다. 이는 실사용 상한이 아니라 초기 설정이며 앱에 맞게 조정해야 한다. 현재 Emscripten dev 문서에서는 `ALLOW_MEMORY_GROWTH` 기본값이 false이고, growth를 켜도 `MAXIMUM_MEMORY` 기본값은 2 GiB다. Qt가 생성하는 실제 linker 설정과 이 기본값은 반드시 Qt가 고정한 Emscripten 4.0.7에서 다시 확인해야 한다. 어느 경우든 현재 4 GiB resident budget과 3.75 GiB 단순 프레임 집합은 기본 웹 설정과 양립하지 않는다([Q1](https://doc.qt.io/qt-6/wasm.html), [E4](https://emscripten.org/docs/tools_reference/settings_reference.html)).

웹 셸에는 별도 정책이 필요하다.

- 초기 새 문서 상한: 데스크톱 2048×2048, 모바일 beta 1024×1024
- 기본 문서: 1024×768 이하
- 초기 프로젝트 import 경고/거부 기준: 32 MiB
- history: 32~64 MiB, preview cache: 3~4 프레임 또는 64 MiB 이하
- 큰 파일은 전체 복제 횟수를 줄이고, frame/raster decoding을 지연
- 4096×4096·60프레임을 지원한다고 표시하지 않음

이 값들은 2~4주 기술 spike에서 1024/2048 캔버스, 긴 stroke, 60프레임 재생, import/serialize peak를 측정한 뒤 결정해야 한다.

### 3.8 플랫폼 종속 코드와 네트워크

플랫폼 코드는 CMake 조건부로 비교적 분명하다.

- macOS: `src/ui/MacWindowChrome.mm`과 AppKit(`cmake/UguruguTargets.cmake:48-61`), Sparkle 2.9.4(`cmake/UguruguDependencies.cmake:89-115`)
- Windows: private Qt/WinTab 경로(`src/main.cpp:44-74,228-239`), Velopack 1.2.0(`cmake/UguruguDependencies.cmake:116-147`)
- 공통 앱 서비스: 단일 인스턴스, 파일 로그, 경로 기반 복구, 창 geometry/state

웹 빌드에서는 updater, AppKit, WinTab, 단일 인스턴스 잠금, 파일 로그, 창/도크 geometry를 제외한다. 브라우저 콘솔 로깅과 웹 자체 버전 표시로 대체한다.

`src`에서 QNetwork, WebSocket, TCP/UDP 사용은 발견되지 않았다. 현재 앱은 updater와 외부 도움말 링크를 제외하면 오프라인이므로 MVP는 외부 서버나 CORS에 의존할 필요가 없다. 향후 API를 추가한다면 실제 게임 iframe의 origin을 서버 allowlist에 넣고 HTTPS/CORS를 구성해야 한다. SharedArrayBuffer opt-in 시에는 COEP `require-corp` 때문에 외부 resource가 추가로 차단될 수 있다.

### 3.9 외부 라이브러리, 빌드와 라이선스

- CMake 3.31+, C++23, AUTOMOC/AUTOUIC/AUTORCC(`CMakeLists.txt:1-24`, `cmake/UguruguBuildSettings.cmake:1-6,67-93`)
- Qt 6.10+ Core/Concurrent/Gui/GuiPrivate/Widgets/LinguistTools/ShaderTools, 배포 Qt 6.11.1(`cmake/UguruguDependencies.cmake:3-30`, `BUILDING.md:3-10`)
- spdlog 1.16.0, libwebp 1.6.0, macOS Sparkle 2.9.4, Windows Velopack 1.2.0(`cmake/UguruguDependencies.cmake:32-147`)
- 현재 `CMakePresets.json:8-243`은 macOS/Windows만 포함하고 Wasm preset이 없다.
- `ugurugu_core`는 Qt Core/Gui, spdlog, webp/webpmux에 연결되고 Widgets는 의도적으로 제외된다(`cmake/UguruguTargets.cmake:9-24`).
- `ugurugu_ui`는 Widgets, Concurrent, GuiPrivate와 QRhi shader를 포함한다(`cmake/UguruguTargets.cmake:26-46`).

다만 현재 `ugurugu_core` 소스 목록에는 문서/렌더뿐 아니라 `ApplicationInstanceLock`, `Logging`, `RecoveryStore/Writer`, `ExportWorker` 같은 데스크톱 서비스도 섞여 있다(`cmake/UguruguSources.cmake:1-18,55-83`). “Widgets에 연결되지 않음”이 곧 “웹 엔진임”을 뜻하지 않는다. 권장 구조에서는 순수 engine/domain/codec와 desktop service를 한 번 더 분리해야 한다.

Qt 6.11.1의 공식 Wasm 도구 조합은 Emscripten 4.0.7이며 ABI 호환 때문에 정확히 맞춰야 한다([Q1](https://doc.qt.io/qt-6/wasm.html), [Q2](https://doc.qt.io/qt-6/supported-platforms.html)). Qt Wasm은 기본적으로 정적 링크다. 프로젝트가 GPL-3.0-or-later이므로 Qt의 GPLv3 경로와 방향은 맞지만, WebAssembly 배포 시 대응 소스, 저작권·라이선스 고지, 빌드 재현 정보 등 실제 배포 의무는 출시 전 별도 검토해야 한다. 이 보고서는 법률 의견이 아니다.

## 4. itch.io HTML5 최신 요구사항

### 4.1 ZIP 구조와 진입 파일

itch.io 공식 Creator 문서 [I1](https://itch.io/docs/creators/html5)의 현재 요구사항은 다음과 같다.

- 여러 파일 프로젝트는 ZIP 하나로 업로드한다. RAR, 7z, tar.gz는 지원하지 않는다.
- ZIP에는 진입점인 `index.html`이 포함되어야 한다.
- 공식 문구는 “ZIP에 포함”이라고 하며 루트 디렉터리라고 명시하지는 않는다. 운영상 모호성을 없애기 위해 ZIP 루트에 두는 것을 권장한다.
- 모든 런타임 파일을 ZIP에 포함하고 asset 경로는 상대 경로를 사용한다. `/assets/...` 같은 절대 경로는 프로젝트의 CDN 하위 경로를 벗어나 실패한다.
- 파일명은 대소문자를 구분하고 UTF-8이어야 한다.
- 단일 self-contained HTML은 그대로 올릴 수 있지만, Wasm/JS/font를 분리하는 이 프로젝트에는 ZIP이 적합하다.

권장 배포 ZIP 예시는 다음과 같다.

```text
index.html
assets/
  app-[hash].js
  app-[hash].css
  ugurugu-[hash].wasm
  engine-worker-[hash].js
  PretendardJP-Medium-[hash].woff2
```

Qt/Emscripten이 생성한 loader가 있더라도 itch.io 진입 파일 이름은 `index.html`로 맞추고, base URL과 worker/Wasm URL이 모두 상대 경로인지 패키지 검사에서 확인해야 한다.

### 4.2 업로드·파일 크기 제한

현재 HTML5 ZIP의 공식 제한은 다음과 같다([I1](https://itch.io/docs/creators/html5)).

| 항목 | 현재 제한 |
|---|---:|
| 압축 해제 후 파일 수 | 최대 1,000개 |
| 경로를 포함한 파일명 길이 | 최대 240자 |
| 압축 해제 후 전체 크기 | 최대 500 MB |
| 압축 해제 후 단일 파일 크기 | 최대 200 MB |

Creator 문서에는 ZIP 자체의 압축된 업로드 크기 상한이 별도로 명시되어 있지 않다. Butler 일반 backend는 압축 해제 총 30 GB까지 받는다고 설명하지만([I2](https://itch.io/docs/butler/pushing.html)), 이는 HTML5의 500/200 MB 규칙을 완화하지 않는다. 2026년 itch.io 관리자의 답변도 Butler가 HTML5 제한을 바꾸지 않는다고 확인한다([I7](https://itch.io/t/5864706/zip-contains-file-that-is-too-large-butler)).

itch.io는 이 수치를 “default limits”라고 부르며 적절하지 않은 경우 support에 문의할 수 있다고 명시한다. 계정별 예외 가능성은 있지만 승인 전에는 제품·bundle 설계의 상한으로 간주하고, 예외 승인을 전제로 일정을 세우지 않는다.

2022년 관리자 포럼 답변에 나온 “일반 웹 업로드 기본 1 GB”는 현재 Creator 문서의 보장값이 아니며 계정별 편집 화면에서 달라질 수 있으므로 과거 참고로만 다룬다([I6](https://itch.io/t/1925786/what-is-the-maximum-file-size-you-can-put-in-itchio)). 지원팀의 별도 승인이 없는 상태에서 적용할 기본 HTML5 상한은 전체 500 MB/단일 200 MB다. 한 개의 Wasm 파일이 200 MB를 넘으면 Butler만으로 해결되지 않는다.

itch.io CDN은 확장자가 `wasm`인 파일을 포함해 HTML/JS/CSS 등을 자동 gzip 압축하고, `.br` 사전 압축 파일도 인식한다([I1](https://itch.io/docs/creators/html5)). 릴리스 빌드에는 LTO/size 최적화와 Brotli를 검토하되, 실제 CDN의 `Content-Encoding`/`Content-Type`을 staging에서 확인한다.

### 4.3 실행 모델: iframe, 페이지 내 실행, 전체 viewport

업로드한 앱은 itch.io 프로젝트 페이지의 iframe에서 실행된다. 표시 방식은 두 가지다([I1](https://itch.io/docs/creators/html5)).

| 방식 | 동작 | Ugurugu 평가 |
|---|---|---|
| 페이지 내 실행 | 지정한 고정 viewport 크기의 iframe에서 실행 | 데스크톱 짧은 체험에는 가능하지만 작업 영역·도킹 UI에는 좁고 포커스 이슈가 더 잘 드러남 |
| 클릭 후 전체 viewport 실행 | Launch 클릭 후 전체 viewport를 사용하고 크기는 동적 | 편집기형 앱에 더 적합하며 권장 기본값 |

`Click to Play`가 기본이며 초기 페이지 성능 저하를 막는다. 이 시작 클릭의 transient user activation이 비동기 Wasm 로딩 뒤까지 유지되거나 cross-origin iframe 내부 API에 전달된다고 가정하면 안 된다. 파일 picker, clipboard, 앱 자체 fullscreen은 각각의 앱 UI에서 별도 사용자 제스처로 호출한다. itch.io의 Fullscreen Button은 페이지 내 실행 모드에서 overlay할 수 있고 scrollbar는 기본적으로 숨겨진다. 모바일에서는 설정과 무관하게 클릭 후 전체 viewport 모드로 실행된다. 따라서 웹 UI는 고정 1280×820을 가정하지 않고 `resize`, orientation, device pixel ratio 변경에 대응해야 한다.

“전체 viewport launch”와 브라우저 Fullscreen API는 구분해야 한다. 전자는 itch.io의 실행 방식이고, 후자는 사용자 활성화·iframe 권한·브라우저 정책이 적용되는 API다. 캔버스 크기는 둘 다 대응해야 한다.

### 4.4 WebAssembly·그래픽·Worker 지원

| 기능 | itch.io/브라우저 상태 | 이 프로젝트의 결정 |
|---|---|---|
| WebAssembly | itch.io는 `.wasm`을 정적 asset으로 제공하고 자동 gzip 대상으로 명시 | 사용 |
| WebGL 1/2 | 브라우저에서 널리 제공되고 Qt Wasm이 요구함. 장치/GPU denylist나 context loss 가능 | WebGL 2 기준. Canvas 2D는 표시 폴백일 뿐 Qt runtime의 WebGL 요구를 없애지는 않음 |
| WebGPU | itch.io의 별도 보장 없음. secure context에서만 가능하고 브라우저 범위가 아직 균일하지 않음 | MVP 제외, 향후 feature detection |
| 일반 Web Worker | 같은 origin의 상대 URL worker는 iframe에서도 사용 가능 | API는 사용 가능. Qt engine 전체의 Worker 구동은 별도 gate |
| Wasm Pthreads | Worker뿐 아니라 SharedArrayBuffer와 cross-origin isolation 필요 | MVP 제외 |
| OffscreenCanvas | Worker rendering 최적화에 유용하나 환경별 확인 필요 | feature detection 후 사용, 폴백 유지 |

Qt 6.11은 Core, GUI, Widgets, Concurrent를 Wasm 지원 모듈로 나열하지만 Qt 자체가 WebGL을 요구하며 WebGL 2 장치를 권장한다([Q1](https://doc.qt.io/qt-6/wasm.html)). 현재 QRhiWidget 경로가 Qt Wasm에서 지원된다는 명시적 보장은 없으므로 1번 방향의 첫 번째 검증 항목이다.

WebGPU는 itch.io가 차단한다고 볼 근거도 없지만, itch.io가 호환성을 보장한다고 볼 근거도 없다. 현재 브라우저 API 상태와 모바일 범위를 고려하면 Ugurugu의 CPU renderer 결과를 올리는 데 WebGL 2로 충분하다([B15](https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API), [B16](https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API)). 일반 Worker entry는 same-origin URL이어야 하므로 ZIP 내부의 상대 URL로 제공하고, 실제 iframe origin에서 생성되는지를 검사한다([B18](https://developer.mozilla.org/en-US/docs/Web/API/Worker/Worker)).

### 4.5 SharedArrayBuffer, COOP/COEP와 iframe

Emscripten Pthreads는 `SharedArrayBuffer`를 사용하며 COOP/COEP header가 정확해야 한다([E1](https://emscripten.org/docs/porting/pthreads.html), [B1](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer)).

itch.io에서는 프로젝트 편집 화면의 Frame Options에서 **SharedArrayBuffer support (Experimental)**를 켜는 opt-in 방식이다([I3](https://itch.io/t/2025776/experimental-sharedarraybuffer-support)).

- 별도 CDN domain `html.itch.zone`을 사용한다.
- 게임 HTML에는 `Cross-Origin-Opener-Policy: same-origin`이 적용된다.
- 파일에는 `Cross-Origin-Embedder-Policy: require-corp`와 `Cross-Origin-Resource-Policy: cross-origin`이 적용된다.
- 게임을 포함하는 itch.io 프로젝트 페이지에는 `COOP: same-origin`과 `COEP: credentialless`가 적용된다.
- 공식 Game Embed 페이지에는 `COOP: same-origin`과 `COEP: require-corp`가 적용된다.
- origin이 바뀌므로 이전 origin의 localStorage를 그대로 볼 수 없다.
- 외부 domain resource는 CORS/CORP header가 맞지 않으면 차단된다.
- 프로젝트 페이지의 다른 third-party iframe도 깨질 수 있다.

2026년 itch.io 관리자 업데이트에 따르면 Firefox는 더 이상 별도 창이 필요하지 않지만 Safari는 여전히 별도 창이 필요하다([I4](https://itch.io/post/16079334)). 이 내용은 2026년의 실제 운영 정보이지만 기능 이름 자체가 Experimental이고 Creator 공식 문서에는 안정 기능으로 편입되지 않았다. 그러므로 `crossOriginIsolated`를 staging에서 확인하지 않은 채 필수 실행 조건으로 삼아서는 안 된다.

Worker gate가 통과한 권장 아키텍처는 일반 Dedicated Worker만 사용하므로 SharedArrayBuffer가 필요 없다. 향후 Pthreads를 추가한다면 threaded/non-threaded artifact를 각각 빌드하고 loader에서 기능 감지해야 한다. Emscripten은 한 바이너리에서 스레드 사용과 단일 스레드 폴백을 동시에 제공할 수 없다고 명시한다.

### 4.6 파일 열기·저장·다운로드·영속 저장

브라우저에는 임의 로컬 경로를 동기식으로 여는 기능이 없다.

- 기준 열기 경로: `<input type="file">`, drag-and-drop의 `File`
- 기준 저장 경로: 메모리의 바이트를 `Blob`으로 만들고 `download` 링크로 내보내기
- 향상 기능: `showOpenFilePicker()`/`showSaveFilePicker()`가 있고 현재 launch context에서 허용되는 경우에만 핸들 유지
- 임시 Wasm filesystem: MEMFS는 reload 후 사라짐
- 장기 저장: IndexedDB 또는 IDBFS가 필요하며 동기화는 비동기

File System Access API는 MDN에서 Baseline이 아니며, picker는 사용자 활성화뿐 아니라 same-origin policy 때문에 차단되어 `SecurityError`를 낼 수 있다. itch.io의 cross-origin iframe에서는 API 존재 여부만으로 사용 가능하다고 판단하지 말고 페이지 내/전체 viewport 모드를 각각 실행 시험해야 한다. 따라서 유일한 저장 경로로 사용할 수 없다([B8](https://developer.mozilla.org/en-US/docs/Web/API/Window/showOpenFilePicker)). 자동 저장은 IndexedDB에 수행하되, quota는 브라우저·장치·사용 상태에 따라 달라지고 best-effort data는 축출될 수 있다([B11](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria)). Safari 17 이후 cross-origin iframe 저장소는 top-level origin보다 작은 별도 quota를 가질 수 있다([B12](https://webkit.org/blog/14403/updates-to-storage-policy/)).

따라서 “자동 저장됨”을 로컬 파일 저장과 동일시하지 않는다. 명시적 `.ugu` 다운로드를 권위 있는 백업 경로로 두고, 저장 실패/quota 초과를 사용자에게 표시해야 한다.

### 4.7 클립보드

Clipboard API는 HTTPS, 사용자 활성화, permission과 iframe Permissions Policy 영향을 받으며 브라우저 동작이 다르다([B10](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API)). Qt Wasm도 text, URL, 알려진 파일 형식과 image는 다루지만 arbitrary `application/octet-stream`은 지원하지 않는다고 명시한다([Q1](https://doc.qt.io/qt-6/wasm.html)).

itch.io 공식 HTML5 문서는 clipboard-read/clipboard-write 권한 위임을 보장하지 않는다. 포럼의 성공·실패 사례는 환경 관찰일 뿐 계약이 아니다. 따라서 현재 custom MIME 기반 선택 영역 복사를 웹 필수 기능으로 이식하면 안 된다.

권장 우선순위는 다음과 같다.

1. 같은 세션 내 Ugurugu 자체 복사 버퍼
2. 사용자 클릭/키 입력 안에서 표준 PNG 또는 text clipboard write 시도
3. 실패하면 “클립보드 권한 없음”과 `.ugu`/PNG 다운로드 제공
4. paste는 표준 이미지 또는 앱 자체 버퍼부터 지원

### 4.8 모바일 브라우저와 메모리

itch.io는 제작자가 실제 모바일 동작을 확인한 경우에만 Mobile Friendly를 켜라고 안내하며, 모바일에서는 항상 클릭 후 전체 viewport로 실행한다([I1](https://itch.io/docs/creators/html5)). Qt도 mobile Safari/Android Chrome에서 실행할 수 있다고 설명하지만, 지원 플랫폼 문서는 일부 모바일 브라우저가 신뢰성 있게 실행하는 데 필요한 기능을 빠뜨릴 수 있어 포괄적 시험을 권장한다([Q1](https://doc.qt.io/qt-6/wasm.html), [Q2](https://doc.qt.io/qt-6/supported-platforms.html)).

고정된 “모바일 Wasm 최대 메모리”는 없다. 장치 RAM, 브라우저, 탭 수, GPU texture, Wasm linear memory, JS heap, IndexedDB 복제량이 함께 영향을 준다. 실제 한계보다 먼저 탭이 종료될 수 있다. 그러므로 초기 모바일 범위는 다음처럼 축소해야 한다.

- viewer/간단 드로잉 beta
- 1024×1024 이하 새 문서
- 한 손가락 드로잉과 두 손가락 pan/zoom을 명확히 분리
- hover, 우클릭, 펜 side button을 필수로 하지 않음
- 화면 회전, safe area, virtual keyboard, background/resume 시험
- 2048 이상, 긴 애니메이션, GIF/WebP export는 기능 감지와 메모리 검사 후 제한

Mobile Friendly 표시는 iOS Safari와 Android Chrome 실제 장치 행렬을 통과한 뒤 켠다.

### 4.9 네트워크, 외부 서버, CORS와 보안

itch.io는 HTTPS로 서비스되므로 외부 요청도 HTTPS여야 하고 HTTP resource는 mixed content로 차단된다([I1](https://itch.io/docs/creators/html5), [B14](https://developer.mozilla.org/en-US/docs/Web/Security/Mixed_content)). 다른 origin의 API/asset에는 서버의 CORS 허용이 필요하다([B13](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS)).

- 서버는 프로젝트 페이지 `itch.io` origin이 아니라 staging에서 관찰한 실제 게임 iframe origin을 허용해야 한다.
- credential 요청에는 wildcard `Access-Control-Allow-Origin: *`을 사용할 수 없고 third-party cookie 정책도 별도로 작용한다.
- Worker/Wasm/font URL은 ZIP 내부 상대 경로로 두면 같은 origin으로 제공된다.
- SharedArrayBuffer opt-in의 `COEP: require-corp`는 단순 CORS보다 외부 asset 제약을 강화한다.
- 직접 CDN URL을 복사해 실행하는 방식은 itch.io가 지원 경로로 보장하지 않는다. 프로젝트 페이지 또는 공식 embed를 사용한다.

현재 앱은 외부 네트워크가 필요 없으므로 MVP에서는 서버를 추가하지 않는 것이 가장 안전하다. 향후 cloud sync나 telemetry를 추가할 때 별도의 privacy/CORS/offline 설계를 한다.

## 5. 현재 정보와 오래된 정보 구분

| 항목 | 현재 적용할 정보 | 오래되었거나 제한적으로만 참고할 정보 |
|---|---|---|
| HTML ZIP 파일 수 | 2026-08-06 Creator 문서의 1,000개 | 과거 글의 500개 |
| HTML 용량 | 압축 해제 전체 500 MB, 단일 200 MB | Butler의 일반 30 GB를 HTML에도 적용하는 해석 |
| 일반 웹 업로드 | 계정 편집 화면과 현재 지원 답변을 확인 | 2022 포럼의 기본 1 GB를 영구 보장으로 사용 |
| SharedArrayBuffer | Experimental opt-in, staging에서 `crossOriginIsolated` 확인 | 2022 본문의 “Chrome만 페이지 내 실행” 문구. 현재 글에서 취소선 처리됨 |
| Firefox/Safari | 2026 관리자 답변: Firefox 별도 창 불필요, Safari는 필요 | 두 브라우저 모두 항상 별도 창이 필요하다는 과거 설명 |
| Butler | 현재 일반 backend 30 GB, rsync/Brotli 후 backend bsdiff/Brotli, 같은 channel update | 과거 Butler 용량·패치 수치를 현재 HTML 한도로 사용 |
| Butler hidden push | 새 channel 최초 push에만 `--hidden` 사용, 기존 channel에는 오류 | hidden channel을 반복 internal 배포 환경으로 간주 |
| 복수 HTML5 실행판 | 현재 UI에서 재확인하고 안정판/시험판은 별도 프로젝트 페이지로 운영 | 2020 관리자 답변의 단일 active 제한을 최신 공식 보장으로 단정하거나, 반대로 한 페이지의 복수 active를 가정 |
| Butler UI | itch app 26.12.0 이상은 2026-05부터 GUI push 제공 | Butler는 CLI로만 쓸 수 있다는 과거 설명 |
| Qt Wasm 도구 | Qt 6.11.1 + Emscripten 4.0.7 | Qt 5/초기 Qt 6 loader, module, 지원 브라우저 정보 |
| Emscripten 문서 버전 | 프로젝트 빌드는 Qt가 지정한 4.0.7에서 flag를 검증 | 온라인 dev 문서 제목의 6.0.7-git 설정을 무조건 복사 |
| Emscripten Pthreads | COOP/COEP+SAB 필요, 주요 브라우저에서 사용 가능 | 같은 현재 페이지 하단에 남은 “Firefox Nightly only” 문장. 앞부분과 현대 브라우저 상태에 모순되는 오래된 잔여 문구 |
| WebGPU | 현재 MDN의 Limited availability를 기준 | 과거 Safari Technology Preview/초기 Chrome 지원표를 현재 baseline으로 사용 |

## 6. 구현 방향 비교

### 6.1 요약 비교

재사용률은 현재 제품 소스의 의미 있는 로직을 기준으로 한 추정치이고, 인월은 숙련 C++/웹 개발자 한 명의 전일제 한 달이다. PoC 전 오차는 ±30~50%다.

| 방향 | 코드 재사용률 | 개발 난이도 | 성능 | 번들 | 유지보수 | itch.io 적합성 | 모바일 | 장기 확장 |
|---|---:|---|---|---|---|---|---|---|
| 1. 전체 Qt for Wasm | 75~90% | 중상~높음 | CPU 렌더는 양호, UI/스레드 위험 | 큼 | 단일 UI 장점, 웹 예외 누적 | 중간 | 낮음 | 중하 |
| 2. C++ 직접 Emscripten | 20~35%* | 매우 높음 | 잘 설계하면 높음 | 작음~중간 | 중간 | 높음 | 높음 | 높음 |
| 3. Qt Core/Gui Wasm + TS UI | 45~55% | 높음 | 높음, worker/전송 검증 필요 | 중간 | 가장 균형이 좋음 | 높음 | 중상~높음 | 높음 |
| 4. 별도 웹 클라이언트 | 5~20% | 최소판 중간, 동등성 매우 높음 | 구현에 따라 높음 | 작음 | 중복·포맷 drift 위험 큼 | 높음 | 높음 | 중간 |

\* 2번에서 Qt Core/Gui를 유지하면 사실상 3번의 UI 선택만 달라진다. 표의 수치는 Qt를 제거하고 일반 C++/웹 API로 직접 이식하는 경우다.

현재 Wasm build가 없으므로 압축 bundle의 절대 크기를 근거 있게 제시할 수는 없다. Qt Wasm은 Qt와 앱을 정적으로 한 Wasm에 넣고, 현재 원본 `PretendardJP-Medium.otf`만 약 3.6 MB다(`cmake/UguruguTargets.cmake:114-118`). 따라서 bundle 순위는 Widgets/Concurrent/GuiPrivate와 데스크톱 resource까지 포함하는 1번이 가장 크고, 이를 제외하는 3번이 그다음이며, Qt를 제거하는 2/4번이 가장 작을 가능성이 높다. Qt 문서의 소형 예제 크기를 이 앱의 예상치로 전용하지 않고 단계 1에서 Wasm, JS, font의 원본/gzip/Brotli 크기를 각각 측정한다.

### 6.2 1번: 전체 Qt 애플리케이션을 Qt for WebAssembly로 이식

장점:

- `ugurugu_ui`를 포함해 가장 많은 코드를 재사용한다.
- 데스크톱 브라우저에서 현재 기능을 빨리 시연할 가능성이 가장 높다.
- Qt의 loader, WebGL 표시, file dialog 웹 API를 이용할 수 있다.

문제:

- 최소 900×640의 도킹 UI와 데스크톱 중심 캔버스 제스처는 모바일에 맞지 않는다.
- 10개 모달 `exec()`와 13개 동기식 선택 API를 비동기 상태 머신으로 바꿔야 한다. Asyncify는 bundle/CPU overhead가 있고 JSPI는 Qt 소스 build와 브라우저 범위 제약이 있어 전체 UI의 영구 우회책으로 권장하지 않는다.
- 항상 시작되는 Export/Recovery QThread와 QtConcurrent 경로를 단일 스레드화하거나 itch.io Experimental SAB에 의존해야 한다.
- `CanvasFrameView`의 QRhi/GuiPrivate 웹 지원이 불명확하다.
- Qt Widgets 전체와 3.6 MB 원본 font resource가 정적 Wasm에 포함되어 초기 다운로드/컴파일 비용이 크다.
- 접근성은 Qt Wasm에서 기본 수준이며 tree/table 같은 복합 UI는 누락 가능성이 있다.
- 웹과 데스크톱의 상호작용 관습이 달라 `#ifdef EMSCRIPTEN`과 특수 흐름이 UI 전반에 누적될 수 있다.

평가: 2~4주짜리 “전체 앱이 뜨는가” 기술 시험에는 가치가 있지만, itch.io 공개판의 장기 구조로는 권장하지 않는다. 데스크톱 브라우저 MVP 4~7 인월, 제한적인 모바일/브라우저 안정화를 포함한 공개 품질 8~15 인월을 예상한다.

### 6.3 2번: Emscripten으로 기존 C++을 직접 WebAssembly로 컴파일

Qt를 유지한 채 Emscripten만 직접 쓰면 `QImage`/`QPainter`와 이벤트 처리를 위해 Qt Wasm runtime이 다시 필요하므로 1번 또는 3번으로 수렴한다. Qt를 제거한다면 다음을 교체해야 한다.

- `Document` 전반의 Qt container/value type
- QObject signal/history controller
- QImage/QPainter composition, blend, transform와 image codec
- QJsonDocument, QByteArray, qCompress 기반 serializer
- QString/translation과 각종 Qt utility

알고리즘 일부와 스키마 의미는 재사용할 수 있지만 소스 재사용률은 20~35%로 낮아진다. 장기적으로 작은 번들과 자유로운 runtime을 얻을 수 있으나, 현재 프로젝트의 가장 값비싼 자산인 결정적 렌더/파일 호환을 다시 검증해야 한다. MVP도 12~20 인월 규모로 추정하며, 번들 축소가 이 비용을 정당화할 근거가 현재는 없다.

### 6.4 3번: 핵심 로직은 WebAssembly, UI는 TypeScript

장점:

- 기존 `ugurugu_core`의 의도된 비-Widgets 경계를 발전시킨다.
- 문서/히스토리/브러시/CPU 렌더/serializer를 한 구현으로 유지한다.
- 웹의 비동기 파일, IndexedDB, Pointer Events, 반응형 레이아웃을 자연스럽게 사용한다.
- Qt engine Worker gate가 통과하면 일반 Worker로 main thread를 보호하면서 SAB를 피할 수 있다.
- WebGL 2, Canvas 2D, WebGPU 등 프레젠테이션을 C++ 엔진과 독립적으로 발전시킬 수 있다.
- 모바일 UI를 데스크톱 도킹 UI와 별도로 최적화할 수 있다.

비용:

- UI 약 22.9k행은 실질적으로 새로 설계한다.
- C++↔JS command/state 경계와 메모리 소유권을 엄격히 설계해야 한다.
- Wasm worker에서 Qt Core/Gui 및 QImage/QPainter가 headless로 동작하는지 PoC가 필요하다.
- dirty rectangle을 전달할 때 복사·GPU upload 비용을 측정해야 한다.
- desktop과 web의 기능 동기화를 계약 테스트로 관리해야 한다.

평가: 초기 비용은 1번보다 크지만 공개 itch.io판, 모바일, 접근성, 향후 PWA/standalone host까지 고려할 때 가장 균형이 좋다. **권장 방향이다.**

### 6.5 4번: 별도 웹 클라이언트, 파일 포맷과 일부 로직만 공유

웹을 TypeScript/Rust 등으로 독립 구현하면 초기 UI와 bundle은 가장 웹답게 만들 수 있다. 그러나 파일 포맷과 렌더 의미를 별도 구현하면 다음 위험이 크다.

- schema 13 이후 변경을 두 serializer에서 동시에 구현
- 같은 stroke가 데스크톱과 웹에서 다른 pixel을 생성
- blend/mask/selection/wobble 동작 drift
- golden test와 버그 수정의 이중화

“간단 viewer/체험판”만 만든다면 6~10 인월에 가능할 수 있지만, 폭넓은 편집 동등성은 18~30 인월 이상이고 장기 유지보수가 가장 불리하다. C++ Wasm이 기술적으로 실패했을 때의 fallback이지 첫 선택은 아니다.

## 7. 권장 기술 스택과 아키텍처

### 7.1 기술 스택

- C++23
- Qt 6.11.1 Core/Gui의 Wasm 정적 빌드
- Emscripten 4.0.7 고정
- 웹 엔진에서는 Qt Widgets, Qt Concurrent, Qt GuiPrivate 제외
- MVP 웹 엔진에서는 animated WebP가 필요할 때까지 libwebp/webpmux 제외, spdlog는 console adapter로 한정
- TypeScript strict mode, Svelte 5, Vite (2026-08-07 결정으로 React에서 변경)
- 일반 Dedicated Worker 1개를 1차 목표로 하되 Qt Core/Gui worker 초기화는 단계 1의 release gate
- WebGL 2 texture presentation, Canvas 2D 표시 폴백
- OffscreenCanvas는 feature detection 후 최적화
- Pointer Events, ResizeObserver, Page Visibility API
- File API + Blob download 기준, File System Access 선택 기능
- IndexedDB 직접 또는 작은 promise wrapper
- Playwright 기반 브라우저 통합 시험
- 기존 QtTest serializer/renderer golden 시험 유지

웹 font는 현재 OTF를 무조건 그대로 싣지 않고 라이선스를 확인한 뒤 WOFF2 변환과 필요한 문자 subset을 검토한다. 한글·일본어 UI를 지원하므로 무리한 subset이 번역 누락을 만들지 않는지 자동 glyph 검사도 필요하다.

프레임워크를 두는 이유는 canvas 자체를 프레임워크로 그리기 위해서가 아니라 timeline, layer tree, tool property, dialog, responsive panel처럼 상태가 많은 UI를 안정적으로 구성하기 위해서다. Svelte 5를 선택한 근거는 핵심 화면이 어차피 자작 컴포넌트라 React 컴포넌트 생태계의 이점이 제한적이고, runes의 세밀한 반응성이 작은 값이 자주 바뀌는 도구 상태와 Worker state diff 수신 패턴에 맞으며, 1인 개발에서 보일러플레이트가 적다는 점이다. 접근성 프리미티브 생태계는 React(react-aria/Radix)가 더 성숙하므로 대화상자·슬라이더·트리는 bits-ui 또는 WAI-ARIA APG 패턴 직접 구현으로 보완한다. 픽셀 프레임과 고빈도 pointer sampling은 프레임워크 state를 통과시키지 않고 전용 canvas/input controller에서 처리한다.

### 7.2 전체 구조

```text
                 shared source of truth
        ┌──────────────────────────────────┐
        │ ugurugu_engine                   │
        │ Document / history / brush       │
        │ CPU render / serializer / codecs │
        │ Qt Core + Gui only               │
        └──────────────┬───────────────────┘
                       │
          ┌────────────┴─────────────┐
          │                          │
┌─────────▼──────────┐     ┌─────────▼────────────┐
│ Desktop application│     │ ugurugu_web_bridge  │
│ Qt Widgets UI      │     │ versioned C ABI     │
│ desktop services   │     │ Wasm single-thread  │
│ Sparkle/Velopack   │     └─────────┬────────────┘
└────────────────────┘               │ messages/buffers
                            ┌─────────▼────────────┐
                            │ Dedicated Worker*   │
                            │ owns Wasm + document│
                            └─────────┬────────────┘
                                      │ state diff /
                                      │ dirty pixels
                            ┌─────────▼────────────┐
                            │ TypeScript web UI   │
                            │ Svelte 5 + canvas   │
                            │ File/IDB/clipboard  │
                            └──────────────────────┘
```

* 단계 1에서 Qt Core/Gui의 standalone Worker 초기화, font/plugin, `QImage`/`QPainter` 동작을 확인한 경우의 목표 구조다. 실패하면 임시로 main thread에서 짧은 cooperative engine task를 실행하고 UI에 제어를 자주 반환하거나, Qt Gui 의존을 더 줄인 engine build를 검토한다. 어느 대안도 PoC 없이 확정하지 않는다.

Wasm bridge는 Qt C++ ABI나 내부 객체 포인터를 JS에 노출하지 않는다. versioned command와 byte buffer를 받는 좁은 `extern "C"` ABI를 둔다. 제어 메시지는 작은 JSON 또는 명시적 binary struct로 시작할 수 있지만, frame pixel은 typed buffer로 분리한다. Embind는 PoC 속도를 높이는 용도로는 쓸 수 있으나 장기 공개 경계는 C ABI가 버전·예외·소유권을 관리하기 쉽다.

현재 표시 경계는 `QImage::Format_ARGB32_Premultiplied`이고 little-endian scanline은 BGRA byte order다(`src/ui/CanvasFrameView.cpp:168-203`). Web bridge가 이를 단순 “RGBA”라고 부르면 red/blue channel과 alpha 합성 계약이 어긋난다. 단계 1에서 **premultiplied BGRA8+shader swizzle**와 **명시적 premultiplied RGBA8 변환**의 비용을 비교한 뒤 format, byte order, stride, alpha semantics를 ABI 버전에 고정한다.

Worker gate가 통과하면 Worker가 document와 Wasm heap의 유일한 소유자가 된다. UI는 명령을 보내고 state diff를 받는다. 포인터 샘플은 batch로 전송하며, 프레임은 다음 순서로 최적화한다.

1. OffscreenCanvas+WebGL 2가 안정적이면 canvas를 Worker로 넘겨 Wasm heap의 dirty rectangle을 worker 안에서 texture upload
2. 불가능하면 dirty rectangle만 복사해 transferable `ArrayBuffer` 또는 `ImageBitmap`으로 UI에 전달
3. Qt engine이 이미 초기화된 상태에서 프레젠테이션 WebGL context만 실패하면 Canvas 2D로 표시

Qt runtime 자체가 WebGL 부재 때문에 초기화에 실패하면 Canvas 2D 폴백은 사용할 수 없고 unsupported 환경으로 처리한다. “WebGL 없는 전체 앱 지원”은 headless build가 이를 가능하게 한다는 별도 증거가 생기기 전에는 범위에 넣지 않는다.

Wasm linear memory 자체는 일반 Worker에서 UI로 transfer할 수 없으므로 “zero-copy”를 전제로 일정과 성능을 약속하면 안 된다. 2048×2048 4-byte premultiplied pixel buffer는 프레임당 16 MiB이므로 dirty rectangle과 preview 해상도 제한이 핵심이다.

### 7.3 데스크톱과 웹을 함께 유지하는 목표 구조

현재 경로를 개념적으로 다음 책임으로 재분류한다. 이는 권장 구조이며 이 보고서에서는 실제 파일을 이동하지 않았다.

- `ugurugu_engine`
  - `src/document/`
  - `src/brush/`
  - `src/input/`의 플랫폼 독립 stabilizer
  - `src/render/`
  - `src/io/serializer/` 및 바이트 codec
- `ugurugu_desktop_services`
  - `ApplicationInstanceLock`, `Logging`
  - `RecoveryStore/RecoveryWriter`
  - path 기반 load/save와 `ExportWorker`
  - updater
- `ugurugu_ui`
  - 현재 Qt Widgets UI와 `CanvasFrameView`
- `ugurugu_web_bridge`
  - C ABI, command dispatcher, buffer ownership
- `web/`
  - TypeScript UI, Worker loader, WebGL/Canvas presenter
  - browser file/storage/clipboard/input adapter

플랫폼 차이는 engine 곳곳의 `#ifdef`가 아니라 외부 adapter와 target source 목록에서 처리한다. 데스크톱도 가능하면 `DocumentSerializer::toJson/fromJson`과 같은 동일한 바이트 경계를 사용한다.

공유 계약 시험은 다음을 CI gate로 둔다.

- native와 Wasm의 같은 fixture load/save round-trip
- schema version 검증과 합의한 top-level 추가 field 보존
- raster/mask hash 일치
- static fixture는 native/Wasm exact pixel hash, animated/wobble fixture는 Wasm별 baseline 또는 명시한 platform 허용 기준
- history command 후 state digest 일치
- web bridge malformed input/size limit 시험

임의 unknown field 보존은 현재 serializer 전체의 보장 성질이 아니다. `fromJson()`은 요청 시 root object를 별도로 돌려줄 수 있고 추가 root field를 넣는 `toJson()` overload가 있지만, 일반 `toJson(Document)`은 알려진 schema를 다시 구성한다(`src/io/DocumentSerializer.cpp:654-701,704-1005`, `src/io/serializer/DocumentJsonCodec.cpp:1524-1544`). Web bridge가 보존할 top-level field를 명시하고 nested unknown까지 약속할지는 별도 format 결정으로 남긴다.

또한 기존 golden test도 Classic motion의 platform math 차이 때문에 macOS/Windows animated hash를 분리한다(`tests/LegacyRenderGoldenTests.cpp:79-105`). 그러므로 모든 animated frame에 native와 bit-identical hash를 요구하는 것은 현재 코드의 실제 계약보다 강하다.

## 8. 재사용·재설계·축소 범위

### 8.1 그대로 또는 작은 adapter로 재사용할 모듈

| 영역 | 코드 위치 | 예상 재사용 |
|---|---|---|
| 문서 구조와 operation | `src/document/Document.*` 및 관련 operation | 높음 |
| history/controller 핵심 | `src/document/DocumentController.*` | 높음. path/UI signal 외피는 조정 |
| brush preset/engine | `src/brush/` | 높음 |
| stabilizer와 stroke sample | `src/input/` | 높음 |
| CPU 렌더와 합성 | `src/render/RenderEngine*`, `StrokeRenderer*` | 높음 |
| resample/fill/mask | `ImageResampler.*`, `FloodFillMask.*` | 높음 |
| `.ugu` schema/serializer | `src/io/DocumentSerializer.*`, `src/io/serializer/` | byte API는 높음 |
| legacy Wawa reader | `WawaV10Reader/Importer.*` | 후속 단계에서 높음 |
| GIF/WebP encoder 알고리즘 | `GifWriter.*`, `WebPWriter.*` | output sink와 메모리 구조 변경 후 재사용 |
| native golden tests | `tests/DocumentTests.cpp`, `RenderEngineTests.cpp`, `StrokeRenderingTests.cpp`, `LegacyRenderGoldenTests.cpp` 등 | 회귀 기준으로 높음 |

### 8.2 분리하거나 재설계할 모듈

| 영역 | 현재 문제 | 웹 설계 |
|---|---|---|
| MainWindow/도크/대화상자 | Widgets, 고정 최소 크기, 모달 | 반응형 Svelte panel/sheet/dialog |
| CanvasWidget input | QWidget의 mouse/tablet/touch event 상태기 | Pointer Events와 gesture state machine |
| CanvasFrameView | QRhiWidget/GuiPrivate | WebGL 2/Canvas 2D presenter |
| path load/save | QFileDialog/QFile/QSaveFile | File/ArrayBuffer/Blob 비동기 adapter |
| recovery | QStandardPaths, writer QThread | IndexedDB snapshot queue |
| export | 파일 경로, QThread, 전체 frame 보유 | byte/stream sink, worker task, 메모리 상한 |
| clipboard | custom MIME 중심 | app-local buffer + 표준 PNG/text |
| settings | QSettings와 window geometry | local storage/IndexedDB의 웹 설정 |
| lifecycle | singleton, exec 후 shutdown | page visibility/unload를 신뢰하지 않는 저장 모델 |
| memory policy | 4 GiB desktop 예산 | web profile과 작업 전 peak 예측 |
| logging/update | file sink, Sparkle/Velopack | console, build version, itch channel |

### 8.3 웹에서 축소하거나 제외할 기능

MVP에서 제외:

- Sparkle/Velopack updater
- 단일 인스턴스 잠금
- 창/도크 배치 저장과 native title bar
- 회전 파일 로그
- animated GIF/WebP export
- custom binary OS clipboard
- preset 파일 import/export
- Wawa/일반 raster import UI
- 고급 lasso/wand/bucket/selection transform
- layer group 편집의 모든 고급 동작
- 4K·60프레임 지원 약속
- Wasm Pthreads, SharedArrayBuffer, WebGPU

“편집 UI에서 제외”와 “파일을 파괴”는 구분해야 한다. 고급 layer/group/operation이 들어 있는 기존 `.ugu`를 열 경우 engine은 표시와 round-trip을 유지하고, 웹 UI가 지원하지 않는 속성을 저장 과정에서 버리지 않아야 한다. 안전한 round-trip이 입증되지 않은 문서는 읽기 전용 또는 명시적 경고로 열어야 한다.

### 8.4 최소 웹 버전의 현실적인 범위

- 새 문서
- 기존 `.ugu` byte load
- brush와 eraser
- 지원 장치의 pressure
- pan/zoom
- undo/redo
- 기본 color/brush size
- frame 선택과 wobble playback
- 기본 layer 추가/삭제/이름/순서/표시/opacity
- 현재 frame PNG export
- `.ugu` 명시적 다운로드
- IndexedDB recovery slot 1개와 저장 상태 표시
- desktop Chrome/Edge/Firefox/Safari 지원
- iOS Safari/Android Chrome은 beta로 제한된 touch drawing

MVP 성공 기준:

- 별도 itch.io Restricted staging 프로젝트의 페이지 내/전체 viewport 실행에서 모두 시작
- 1024×768 문서 첫 입력 지연과 playback이 사용 가능한 수준
- 2048×2048 스트레스 문서에서 탭 종료 없이 명확한 제한/경고
- desktop과 Wasm의 fixture round-trip, static exact pixel hash, animated/wobble의 합의된 Wasm baseline 일치
- 저장 실패와 IndexedDB quota 오류가 데이터 손실 없이 사용자에게 노출
- 펜 입력 중 브라우저 scroll/zoom과 pointer cancellation을 제어
- 키보드 없이 모든 핵심 기능에 접근 가능

## 9. 단계별 마이그레이션 계획

### 단계 0. 계약과 측정 기준 확정 — 1~2주

- 웹 MVP 기능표와 제외 기능 승인
- document/state command 경계와 ownership 규칙 작성
- native golden fixture, 큰 문서 fixture, legacy fixture 선정
- 성능 지표: startup, Wasm download/compile, stroke latency, frame render, peak memory, save peak 정의

### 단계 1. 수직 기술 spike — 2~4주

가장 먼저 해야 할 작업이다.

- Qt 6.11.1 + Emscripten 4.0.7 single-thread 최소 target
- Dedicated Worker 안에서 Qt Core/Gui, `QImage`, `QPainter` 실행
- `.ugu` bytes load → 현재 ARGB32/BGRA 의미를 보존한 한 frame pixel buffer render → `.ugu` bytes round-trip
- static fixture는 native exact pixel/schema/hash와 비교하고 animated/wobble은 기존 platform별 차이를 반영한 Wasm baseline 작성
- 1024/2048 문서의 peak memory와 dirty upload 비용 측정
- compressed Wasm/JS/font 총량과 초기 시작 시간 측정
- 별도 itch.io Restricted staging 프로젝트에서 embed/full viewport, Worker, file input, Blob download, IndexedDB, pointer/pen, clipboard, `crossOriginIsolated` 기록
- iOS Safari와 Android Chrome 실제 장치 확인
- Qt GPLv3 정적 배포 의무 검토

중단 기준:

- Qt Core/Gui headless worker가 지원 브라우저에서 안정적으로 초기화되지 않음
- native와 Wasm의 렌더/serializer 결정성을 합리적인 범위에서 맞출 수 없음
- 1024 기본 문서가 목표 장치 메모리/지연을 초과함

첫 번째 문제가 해결되지 않으면 Wasm 엔진을 메인 스레드에서 cooperative하게 실행하는 변형을 검토하고, 두 번째까지 실패하면 4번 방향을 재평가한다.

### 단계 2. 엔진 경계와 Wasm bridge — 1.5~2.5 인월

- `ugurugu_core`에서 desktop service 분리
- path가 없는 document/serialize/render API 정리
- versioned C ABI와 오류 모델
- worker command queue, cancellation, state digest
- web memory profile과 문서 사전 검사
- Wasm/native 공통 fixture CI

### 단계 3. 웹 UI와 캔버스 — 3~4.5 인월

- Svelte shell, responsive desktop/mobile layout
- WebGL 2 presenter와 Canvas 2D 표시 폴백
- Pointer Events, pressure, capture, touch gesture
- brush/eraser/color, layers, timeline, undo/redo
- dirty region/state diff 최적화
- browser shortcut 충돌과 focus 처리

### 단계 4. 파일·복구·itch.io 배포 — 1~1.5 인월

- File input, drag/drop, Blob `.ugu`/PNG download
- IndexedDB recovery와 quota/error UX
- build hashing, 상대 경로, size/file-count 검사
- Butler와 별도 Restricted staging 프로젝트, 선택적인 별도 beta 프로젝트
- embed/full viewport 설정과 로딩/오류 화면

### 단계 5. 품질·모바일·접근성 — 2~3 인월

- desktop 4종 브라우저와 iOS/Android 실제 장치
- screen reader, 키보드 전용, reduced motion
- context loss, visibility/background/resume
- malformed/large file와 memory pressure
- 라이선스·소스 배포·third-party notice
- 공개 beta telemetry 없이도 진단 가능한 오류 코드와 build ID

### 단계 6. 후속 동등성

- raster/Wawa import
- 고급 selection과 layer group editing
- streaming GIF/WebP export
- File System Access 향상 기능
- PWA/독립 host
- 측정 결과가 정당화할 때만 SIMD/Pthreads/WebGPU 검토

## 10. 주요 기술 위험과 사전 검증

| 위험 | 가능성 | 영향 | 사전 검증/대응 |
|---|---|---|---|
| Qt Core/Gui+QPainter를 Worker에서 쓰는 미문서화 조합과 Qt의 고정 WebGL 요구 | 높음/미확인 | 매우 큼 | 단계 1에서 headless 초기화, font/plugin과 WebGL 실패 동작을 4개 desktop browser, mobile 2종에서 직접 실행. 실패 시 cooperative main-thread 또는 Qt 의존 축소 재평가 |
| Wasm↔UI frame 복사 비용 | 높음 | 큼 | 1024/2048 dirty/full upload benchmark, OffscreenCanvas와 ImageBitmap 비교 |
| desktop 4 GiB 정책으로 인한 OOM | 높음 | 매우 큼 | web memory profile, import 전 peak 예측, cache hard cap |
| serializer/raster 결정성 차이 | 중간 | 매우 큼 | round-trip fixture, static exact native/Wasm hash, animated/wobble의 별도 Wasm baseline |
| modal/path/thread 코드가 engine에 남음 | 높음 | 큼 | target dependency audit, 웹 engine에서 Widgets/Concurrent/QFile path 금지 |
| itch.io SAB/iframe 운영 변화 | 중간 | 큼 | baseline에서 SAB 제거, 별도 Restricted staging 프로젝트 smoke test |
| iOS 펜/터치/메모리 편차 | 높음 | 큼 | 실제 iPad/iPhone, Android tablet/phone 행렬 |
| IndexedDB quota/eviction | 중간 | 매우 큼 | quota error injection, 다운로드 백업 UX |
| browser shortcut/clipboard 제한 | 높음 | 중간 | 명령 palette/button 경로, app-local clipboard |
| QRhiWidget 웹 미지원 | 1번에서 높음 | 큼 | 1번 PoC gate. 3번에서는 교체 |
| 정적 Qt bundle과 font 크기 | 중간 | 중간 | release LTO/Brotli 측정, WOFF2/subset 검토 |
| GPL 정적 배포 준수 | 낮음~중간 | 매우 큼 | 공개 전 라이선스 checklist와 대응 소스 제공 |

가장 중요한 측정 결과는 평균 FPS 하나가 아니다. stroke 입력 후 화면 반영 p95, frame render p95, full/dirty upload bytes, Wasm+JS peak memory, serialize 시 peak 복제량, background 후 복귀 성공률을 기록해야 한다.

## 11. itch.io 배포와 운영 방식

### 11.1 빌드 산출물 검사

1. release/LTO/size 최적화 build를 생성한다.
2. ZIP 루트에 `index.html`을 둔다.
3. JS/Wasm/Worker/font/asset URL이 모두 상대 경로인지 검사한다.
4. Linux처럼 case-sensitive한 환경에서 참조 대소문자를 검사한다.
5. 압축 해제 후 1,000파일, 240자 path, 전체 500 MB, 단일 200 MB 제한을 CI에서 검사한다.
6. localhost HTTP server에서 MIME, Worker, streaming Wasm load를 확인한다.
7. source map은 공개 artifact에서 분리하거나 민감 정보가 없는지 검토한다.
8. build ID와 파일 format/schema version을 로딩 화면과 오류에 포함한다.

### 11.2 Butler 배포

공식 기본 명령은 다음 형태다([I2](https://itch.io/docs/butler/pushing.html)).

```sh
butler push dist-web user/project-web-staging:html5 --userversion 0.1.0-web.1
```

- directory 또는 ZIP을 push할 수 있다.
- 최초 push 후 itch.io Edit game에서 해당 channel을 “HTML5 / Playable in browser”로 표시한다.
- 프로젝트 Kind를 “HTML”로 설정한다.
- 같은 channel에 다시 push하면 build가 처리된 뒤 업데이트된다.
- `butler push-preview --changes-only ...`로 추가/변경/삭제를 먼저 검토한다.
- 기본 patch는 로컬 rsync-style diff+Brotli이고, 서버가 bsdiff+고품질 Brotli로 다시 최적화한다. 업로드 완료 후 기본 patch 상태로도 live가 된다.
- `--hidden`은 **새 channel을 만드는 최초 push에만** 적용된다. 기존 channel에 `--hidden`으로 다시 push하면 오류가 나므로 반복 staging 수단으로 사용하지 않는다([I2](https://itch.io/docs/butler/pushing.html)).
- 2026-05부터 itch desktop app 26.12.0 이상은 Butler GUI push, 변경 preview, 새 build의 hidden push를 제공한다([I5](https://itch.io/updates/pushing-builds-with-butler-is-now-in-the-itch-app)).

한 프로젝트 페이지에서 internal/beta/stable HTML5 build 세 개를 동시에 실행할 수 있다고 가정하면 안 된다. 2020년 관리자 답변은 한 페이지에서 여러 HTML5 build를 동시에 active로 제공하지 않으며 별도 페이지가 필요하다고 설명했고([I8](https://itch.io/t/974405/does-itch-support-experimental-branches-for-the-playable-web-version-cause-i-cant-seem-find-it)), 현재 Butler 공식 문서도 복수 active playable을 약속하지 않는다. 이 2020 답변은 과거 운영 정보이므로 배포 시점 UI에서 다시 확인하되, 보수적인 운영 구조는 다음과 같다.

- 별도 Restricted 프로젝트 페이지: 반복 가능한 internal staging, `html5` channel
- 선택적인 별도 beta 프로젝트 페이지: 공개 beta, `html5` channel
- 본 프로젝트 페이지: 안정판 하나, `html5` channel

페이지를 추가로 만들지 않으면 한 프로젝트에서 active playable upload를 수동 전환한 뒤 시험하고 되돌려야 한다. channel 이름 자체는 접근 제어가 아니며, 공개 프로젝트의 `html5-internal` 같은 이름만으로 비공개가 되지 않는다.

HTML 웹 실행은 사용자가 매번 최신 static build를 받으므로 데스크톱 updater는 넣지 않는다. 파일 format migration은 build와 독립적으로 backward compatible해야 하며, 새 웹 build를 배포하기 전 이전 `.ugu` fixture를 CI에서 연다. 문제가 생기면 검증된 이전 artifact를 같은 안정 channel에 새 userversion으로 재-push한다.

### 11.3 itch.io 페이지 설정

- 기본 실행은 **Click to launch in fullscreen/전체 viewport**
- 가벼운 demo가 준비되기 전에는 자동 시작을 사용하지 않음
- 페이지 내 embed를 제공한다면 고정 desktop preview로 취급하고 responsive resize를 유지
- scrollbar는 끄고 앱 내부 panel scrolling만 사용
- Mobile Friendly는 실제 장치 gate 이후 활성화
- SharedArrayBuffer option은 baseline build에서 끔
- loader에 다운로드/컴파일 진행률, 메모리 부족, WebGL 실패, unsupported browser 메시지 제공
- 외부 asset/CDN은 사용하지 않고 모든 필수 runtime asset을 ZIP에 포함

### 11.4 릴리스 운영

- 매 릴리스: Butler preview → Restricted staging 프로젝트 → desktop browser smoke → mobile smoke → 선택적 별도 beta 프로젝트 → 본 프로젝트 stable
- staging에서 response header, origin, `crossOriginIsolated`, clipboard/fullscreen availability를 기록
- Wasm/JS/font 압축 크기와 startup p75/p95를 릴리스 간 비교
- 저장 schema가 바뀌면 native와 web을 같은 릴리스 열에 묶어 호환성 표 작성
- itch.io 설정 변경과 브라우저 업데이트를 고려해 분기별 staging 회귀 시험

## 12. 예상 작업 규모

### 권장 3번 방향

| 작업 | 인월 |
|---|---:|
| 계약·측정 기준과 수직 기술 spike(단계 0~1) | 0.75~1.5 |
| 엔진 분리, Wasm target, bridge, 공통 시험 | 1.5~2.5 |
| TS UI, canvas, input, worker, 기본 도구 | 3.0~4.5 |
| 파일/PNG/IndexedDB/itch.io packaging | 1.0~1.5 |
| 성능·메모리·모바일·접근성·브라우저/라이선스 QA | 2.0~3.0 |
| **MVP 합계** | **8.25~13.0** |
| 고급 기능과 데스크톱에 가까운 동등성 추가 | 추가 6~10 |
| **폭넓은 동등성 총합** | **14.25~23.0** |

숙련된 C++/Qt 개발자 1명과 TypeScript/브라우저 개발자 1명이 병행하면 MVP는 약 5~7개월, 폭넓은 동등성은 약 8~12개월의 calendar time이 현실적이다. 전담 QA/디자인이 없거나 mobile Safari 문제, Qt Worker 제약이 발견되면 늘어난다.

방향별 대략적 범위:

- 1번 전체 Qt: desktop browser MVP 4~7 인월, 공개 품질과 제한적 모바일 8~15 인월
- 2번 Qt 제거 직접 이식: MVP 12~20 인월
- 3번 권장 hybrid: 조사·spike 포함 MVP 8.25~13.0 인월, 폭넓은 동등성 총 14.25~23.0 인월
- 4번 별도 웹: 축소 viewer/editor 6~10 인월, 폭넓은 동등성 18~30 인월 이상

이 수치는 서버, 계정 시스템, cloud sync, 협업 편집을 포함하지 않는다.

## 13. 최종 요약

- **itch.io 웹버전 배포 가능 여부:** 가능. 현재 데스크톱 실행 파일의 직접 업로드가 아니라 별도 HTML/TypeScript+WebAssembly 빌드가 필요하다.
- **가장 추천하는 구현 방향:** 3번, 핵심 C++/Qt Core·Gui 로직을 단일 스레드 Wasm으로 재사용하고 UI를 TypeScript로 새로 구현한다.
- **추천 기술 스택:** Qt 6.11.1 Core/Gui + Emscripten 4.0.7 + C ABI + TypeScript/Svelte 5/Vite + WebGL 2 + File/Blob + IndexedDB. Dedicated Worker는 선행 spike 통과 시 사용하고, 실패 시 cooperative main-thread 또는 Qt 의존 축소를 재평가한다.
- **가장 큰 장애물:** 데스크톱의 동기식 대화상자·경로 I/O·항상 실행되는 QThread/QtConcurrent와 4 GiB 메모리 정책을 브라우저의 비동기·제한 메모리 모델로 바꾸는 일이다.
- **가장 먼저 진행해야 할 작업:** 2~4주 수직 spike로 Worker 안의 Qt Core/Gui Wasm이 `.ugu`를 읽고 static fixture의 동일 pixel과 animated/wobble의 일관된 Wasm baseline을 렌더한 뒤 round-trip하는지, 1024/2048 문서의 메모리·전송 비용과 itch.io iframe 입력/저장을 실제 브라우저에서 검증한다.

## 14. 외부 자료 목록

모든 자료의 최종 확인일은 **2026-08-06**이다.

### itch.io

| ID | 상태 | 자료와 URL |
|---|---|---|
| I1 | 현재 공식 | Uploading HTML5 games — <https://itch.io/docs/creators/html5> |
| I2 | 현재 공식 | Pushing builds with butler — <https://itch.io/docs/butler/pushing.html> |
| I3 | 실험적 공식 운영 공지 | Experimental SharedArrayBuffer Support — <https://itch.io/t/2025776/experimental-sharedarraybuffer-support> |
| I4 | 2026 관리자 답변, 운영 현황 | Firefox/Safari 별도 창 상태 — <https://itch.io/post/16079334> |
| I5 | 현재 공식 업데이트 | Pushing builds with butler is now in the itch app, 2026-05-18 — <https://itch.io/updates/pushing-builds-with-butler-is-now-in-the-itch-app> |
| I6 | 과거 참고, 현재 보장 아님 | 2022 일반 업로드 크기 관리자 답변 — <https://itch.io/t/1925786/what-is-the-maximum-file-size-you-can-put-in-itchio> |
| I7 | 2026 관리자 답변 | Butler가 HTML5 제한을 우회하지 않음 — <https://itch.io/t/5864706/zip-contains-file-that-is-too-large-butler> |
| I8 | 2020 관리자 답변, 배포 시 재확인 | 한 페이지에서 복수 HTML5 build 동시 active 미지원 — <https://itch.io/t/974405/does-itch-support-experimental-branches-for-the-playable-web-version-cause-i-cant-seem-find-it> |

### Qt

| ID | 상태 | 자료와 URL |
|---|---|---|
| Q1 | 현재 공식, Qt 6.11 | Qt for WebAssembly — <https://doc.qt.io/qt-6/wasm.html> |
| Q2 | 현재 공식, Qt 6.11 | Supported Platforms / WebAssembly — <https://doc.qt.io/qt-6/supported-platforms.html> |
| Q3 | 현재 공식 | QRhiWidget — <https://doc.qt.io/qt-6/qrhiwidget.html> |
| Q4 | 현재 공식 | QFileDialog — <https://doc.qt.io/qt-6/qfiledialog.html> |

### Emscripten

Emscripten 웹 문서는 확인 시점에 `6.0.7-git (dev)`로 표시되었다. 프로젝트 실제 build flag와 ABI는 Qt 6.11.1이 요구하는 Emscripten 4.0.7에서 다시 검증해야 한다.

| ID | 상태 | 자료와 URL |
|---|---|---|
| E1 | 현재 기술 문서, 일부 하단 문구는 오래됨 | Pthreads support — <https://emscripten.org/docs/porting/pthreads.html> |
| E2 | 현재 기술 문서 | File System Overview — <https://emscripten.org/docs/porting/files/file_systems_overview.html> |
| E3 | 현재 기술 문서 | File System API / IDBFS — <https://emscripten.org/docs/api_reference/Filesystem-API.html> |
| E4 | 현재 기술 문서 | Compiler Settings / memory growth — <https://emscripten.org/docs/tools_reference/settings_reference.html> |
| E5 | 현재 기술 문서 | OpenGL support — <https://emscripten.org/docs/porting/multimedia_and_graphics/OpenGL-support.html> |
| E6 | 현재 기술 문서 | WebGPU support — <https://emscripten.org/docs/porting/multimedia_and_graphics/WebGPU-support.html> |
| E7 | 현재 기술 문서 | Asyncify — <https://emscripten.org/docs/porting/asyncify.html> |

### 브라우저·웹 플랫폼

| ID | 상태 | 자료와 URL |
|---|---|---|
| B1 | 현재 웹 플랫폼 문서 | SharedArrayBuffer security requirements — <https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer> |
| B2 | 현재 웹 플랫폼 문서 | Cross-Origin-Opener-Policy — <https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Cross-Origin-Opener-Policy> |
| B3 | 현재 웹 플랫폼 문서 | Pointer Events — <https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events> |
| B4 | 현재 웹 플랫폼 문서 | CSS touch-action — <https://developer.mozilla.org/en-US/docs/Web/CSS/touch-action> |
| B5 | 현재 웹 플랫폼 문서 | keydown and focus — <https://developer.mozilla.org/en-US/docs/Web/API/Element/keydown_event> |
| B6 | 현재 웹 플랫폼 문서 | Fullscreen request — <https://developer.mozilla.org/en-US/docs/Web/API/Element/requestFullscreen> |
| B7 | 현재 웹 플랫폼 문서 | File API — <https://developer.mozilla.org/en-US/docs/Web/API/File_API> |
| B8 | 현재 웹 플랫폼 문서, Baseline 아님 | File System Access picker — <https://developer.mozilla.org/en-US/docs/Web/API/Window/showOpenFilePicker> |
| B9 | 현재 웹 플랫폼 문서 | HTMLAnchorElement download — <https://developer.mozilla.org/en-US/docs/Web/API/HTMLAnchorElement/download> |
| B10 | 현재 웹 플랫폼 문서 | Clipboard API — <https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API> |
| B11 | 현재 웹 플랫폼 문서 | Storage quotas and eviction — <https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria> |
| B12 | 현재 WebKit 공식 문서 | Safari storage policy update — <https://webkit.org/blog/14403/updates-to-storage-policy/> |
| B13 | 현재 웹 플랫폼 문서 | CORS — <https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS> |
| B14 | 현재 웹 플랫폼 문서 | Mixed content — <https://developer.mozilla.org/en-US/docs/Web/Security/Mixed_content> |
| B15 | 현재 웹 플랫폼 문서 | WebGL API — <https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API> |
| B16 | 현재 웹 플랫폼 문서, Limited availability | WebGPU API — <https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API> |
| B17 | 현재 웹 플랫폼 문서 | WebAssembly.Memory — <https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Memory/Memory> |
| B18 | 현재 웹 플랫폼 문서 | Worker constructor — <https://developer.mozilla.org/en-US/docs/Web/API/Worker/Worker> |
| B19 | 현재 웹 플랫폼 문서 | OffscreenCanvas — <https://developer.mozilla.org/en-US/docs/Web/API/OffscreenCanvas> |
