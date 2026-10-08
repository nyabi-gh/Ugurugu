# Ugurugu 통합 검토·개선 계획

정리일: 2026-10-05 · 제품: **2.2.12** · 코드 기준: main · 데스크톱 성능: `perf/desktop-render-pipeline`(`be2a06d` 기준, 그리기 지연 작업 `c9d6808`까지)은 PR #9로 main에 병합

이 문서는 데스크톱·공용 엔진·웹의 검토 결과, 성능 후속 작업, Android 이식 계획을 합친 **현재 상태의 단일 기준 문서**다. 과거 문서의 발견 번호 `R01–R14`, `D01–D25`는 추적용으로 유지한다. 중복 번호를 독립 결함 수로 합산하지 않는다.

문서 통합 뒤 P1 개선을 시작했다. D04/R01의 bounded decode와 중복 검증 축소, D16의 Sparkle 수정 버전 고정, D05의 최종 저장 경로 확인, D06의 격리된 Windows 패키지 smoke, D01/R02의 변환 합성 수정은 현재 작업 트리에 반영했지만, WASM·macOS·실제 설치 수용 검증은 아직 끝나지 않았다. 2026-10-03에 Windows native 회귀는 실행해 통과했다(아래 각 항목). 역사적 측정과 테스트는 실행 당시의 조건에만 유효하다.

## 1. 핵심 판단

- 공용 문서 모델, 트랜잭션·원자적 저장, 증분 렌더, 취소·세대 관리, 자원 예산과 CI는 유지할 기반이다. 전면 재작성보다 경계별 회귀 테스트와 수정이 우선이다.
- 먼저 해결할 것은 **압축 해제 상한, 업데이트 의존성 보안, 변환·표시 정확성, 저장·복구 경계**다. 대규모 리팩터링이나 신규 플랫폼 UI보다 앞에 둔다.
- 9월 8일 보고서의 네이티브 항목 7개는 관련 코드가 남아 있다. 웹 수동 저장의 큐 우회는 이후 수정됐지만, 자동저장 큐·문서 세대·미저장 문서 교체 문제는 남아 있다.
- 데스크톱 렌더·UI·그리기 지연 개선은 구현과 Windows 측정까지 마쳤다(5절). 남은 것은 다시 구현하는 일이 아니라 macOS·실제 펜·실제 작업 문서에서의 검증이다.
- 웹은 구현·스테이징 기록이 있는 제품이다. Android는 요구사항·계획 단계이며, APK·S Pen·S8 성능이 검증된 제품이 아니다.

### 상태와 우선순위

| 표기 | 의미 |
|---|---|
| 미해결 | 현재 코드에서 해당 경로가 남아 있음. 실행 재현 여부는 근거에 별도 표시 |
| 부분 해결 | 일부 경로만 수정됐거나 구현과 수용 검증 중 하나만 끝남 |
| 검증 대기 | 구현은 있으나 지정한 환경·측정·실기기 완료 근거 없음 |
| 조사·제안 | 성능 가설, UX 선택, 유지보수 개선. 재현 결함과 구분 |
| 계획 | 아직 구현하지 않은 제품 범위·마일스톤 |
| P1 | 데이터 보존·정확성·보안·배포 안전에 우선 대응 |
| P2 | 주요 작업 흐름·입력·접근성·메모리 및 검증 공백 |
| P3 | 측정 후 선택할 최적화·구조 정리·문서 품질 |

이 문서의 소스 링크는 해당 파일을 가리킨다. 함수 이름과 기준 커밋을 함께 사용하며, 변경으로 쉽게 어긋나는 줄 번호만으로 근거를 식별하지 않는다.

## 2. 프로젝트 구조와 검증 기준

| 계층 | 책임·경계 |
|---|---|
| 공용 엔진 | C++23 / Qt Core·Gui. 문서·레이어·획·히스토리·선택 연산·렌더·직렬화·GIF 등. 소스 목록은 [UguruguSources.cmake](../cmake/UguruguSources.cmake) |
| 데스크톱 | Qt Widgets·Concurrent, `CanvasWidget` 입력/프리뷰, `MainWindow` 저장·복구·액션, QRhi 표시 경로. [UguruguTargets.cmake](../cmake/UguruguTargets.cmake) |
| 웹 | Svelte 5 / TypeScript → EngineClient → Worker → C ABI → 같은 엔진. WebGL 표시와 Canvas2D fallback. [App.svelte](../web/src/App.svelte), [BridgeDocument.hpp](../src/wasm/BridgeDocument.hpp) |
| 저장 계약 | 현행 `.ugu` schema 13 / algorithm 3. [SerializerSchema.hpp](../src/io/serializer/SerializerSchema.hpp). Android V1도 이 계약을 유지하는 계획 |
| 도구·의존성 | CMake 최소 3.31, 데스크톱 Qt 최소 6.10 / WASM 최소 6.11, 배포 Qt 6.11.1 고정. Sparkle 2.9.6과 zlib 1.3.2를 hash 고정 |
| 배포 | macOS 서명·공증·Sparkle, Windows Velopack 설치·업데이트, 웹 itch.io 패키지. 빌드 성공과 실제 설치·업데이트 성공은 별도 |

### 증거 범위

| 기록 | 확인된 결과 | 대신하지 못하는 검증 |
|---|---|---|
| 2026-09-08 종합 검토 | macOS Release / Qt 6.11.2, CTest 13/13, 소스 함수·offscreen 집중 재현, 두 크기 UI 캡처 | 배포 Qt 6.11.1 전체 행렬, 실제 펜·GPU·Windows |
| 2026-09-08 웹 기능 추가 | 브라우저 21시나리오·194체크, WASM 스모크, 관련 네이티브 2스위트, Svelte 검사. 당시 완전 패키지 17파일·11.40MiB | 모든 브라우저·모바일·iframe 권한·배포 의무 검토 |
| 2026-09-16 Android 계획 작성 | `73004d7`, macOS Qt 6.11.2 재빌드·13/13, Svelte 오류·경고 0 기록 | Android 빌드·APK·S Pen·S8 실측 |
| 2026-10-03 데스크톱 성능 | Windows 11 x64, VS 18 BuildTools ClangCL(clang-cl 22.1.3), Qt 6.11.2 Release. CTest 13/13. 렌더 A/B는 `ugurugu_render_benchmark`, UI는 offscreen 회귀와 실제 D3D11 창 probe. 세부는 5절 | macOS Metal·Apple Silicon, 실제 펜 입력, 배포 Qt 6.11.1, 사용자 문서 외 실제 작업 문서 |
| 2026-10-05 macOS 데스크톱 | macOS 27.0.1 arm64, Apple clang 21, Homebrew Qt 6.11.2 Debug/Release. offscreen CTest 13/13, UI 5개 스위트를 `QT_QPA_PLATFORM=cocoa`(Metal 표시)로 실행해 실패 0. Release 설치 번들과 `ugurugu_package_smoke_test` 통과. 수정 전후 비교는 별도 worktree에서 같은 설정으로 실행 | 배포 Qt 6.11.1(aqt), Windows 빌드·패키지(이번 변경 중 Windows 전용 코드는 이 환경에서 컴파일하지 않음), WASM(툴체인 없음), 실제 펜·앱 수동 조작 |
| 2026-09-18 현재 작업 | bounded decode·Sparkle 2.9.6·최종 저장 경로 확인·Windows 패키지 격리 smoke 반영. Svelte 검사 0 오류·0 경고, 웹 production build와 itch.io 패키지 검사 통과. Windows CMake에서 zlib 1.3.2 구성과 Clang 22 식별까지 확인 | Qt 6 개발 패키지 부재로 네이티브 build·CTest·실제 Windows 패키지 smoke 미실행. WASM·macOS package/update·새 peak memory 측정 미실행 |

9월 8일 초기 검토의 12파일·0.79MiB 웹 빌드는 **WASM 없는 셸**이었다. 이후 실엔진 패키지 기록과 혼동하지 않는다. 13은 CTest 스위트 수이지 개별 테스트 수가 아니다.

`e883cdd` 이후 변경을 “저작권과 테스트 하나뿐”으로 기술했던 것은 정정한다. `fd99d9a`에는 웹 텍스트·이미지·레이어 모션과 저장 큐 수정, `TextStrokeBuilder::buildFromPath` 추출 및 테스트가 있다. `73004d7`에는 저장소 URL 변경도 있다. 변경량만으로 모든 과거 결함의 존속을 판단하지 않고 아래 경로를 개별 대조했다.

## 3. 데이터·보안·저장 경계 — 먼저 처리

### D04 / R01 · P1 · 실제 압축 해제 출력 상한과 반복 검증 — 부분 해결

근거: [RasterAssetTable.cpp](../src/io/serializer/RasterAssetTable.cpp)의 래스터 등록, [DocumentJsonCodec.cpp](../src/io/serializer/DocumentJsonCodec.cpp)의 마스크 decode, [DocumentSerializer.cpp](../src/io/DocumentSerializer.cpp)의 `fromJson`·`prepare`, [DocumentValidation.cpp](../src/io/serializer/DocumentValidation.cpp).

기존 `qUncompress` 경로는 헤더의 예상 크기를 확인해도 실제 출력량을 제한하지 못했다. 과거 probe에서는 8,168바이트 압축 입력이 8,388,608바이트로 해제된 뒤 거절됐고, 작은 예산에서도 사후 거절 전 RSS 증가가 관찰됐다. 이는 제한 우회 근거이지 원격 코드 실행이나 실제 제품 OOM 재현은 아니다. [Qt qUncompress 계약](https://doc.qt.io/qt-6/qbytearray.html#qUncompress).

현재 작업 트리는 [BoundedCompression.cpp](../src/io/serializer/BoundedCompression.cpp)에서 zlib 1.3.2의 `inflate` 출력 버퍼를 예상 크기로 먼저 고정한다. qCompress 헤더, 정확한 출력 길이, 전체 입력 소비, 정상 스트림 종료를 모두 확인하며 래스터와 세 마스크 decode 경로의 직접 `qUncompress` 사용을 제거했다. zlib는 데스크톱과 WASM 공통 CMake 경로에 정적 연결하고 패키지 고지를 추가했다.

래스터 테이블은 `maximumRasterAssets` 개수 상한을 decode 전에 검사한다. `fromJson` 최초 등록의 검증 결과를 정규화 전·후 문서 검증에서 다시 대조하므로 그 구간의 decode/hash는 3회에서 1회로 줄었다. 일반 열기의 controller `prepare` 검증은 신뢰 경계를 유지해 한 번 더 수행하므로 전체는 4회에서 2회가 됐다.

- 구현됨: oversized·truncated·trailing stream과 래스터·현행/구형 clip mask·binary mask, 자산 개수 상한 회귀를 추가했다.
- 추가 조사: Wawa PNG는 전체 decode 전에 크기·비용을 검사할 수 있는지 확인한다. `QImageReader` 64MiB 한도는 고비트 심도 입력과 배포/테스트 환경 차이를 별도 검증한다.
- 확인 (2026-10-03, Windows 11 x64, Qt 6.11.2 Release(ClangCL)): `rejectsCompressedStreamsLargerThanTheirDeclaredOutput`·`boundsDecodedOutputAndRequiresTheWholeStream`·`rejectsTooManyRasterAssetsBeforeDecodingThem` 통과, `document` 스위트 전체 204건 통과, offscreen CTest 13/13. 수정 전 코드에서 실패하는지는 다시 확인하지 않았다.
- 남음: WASM build/parity, 퍼저 corpus·peak memory·decode 횟수 비교 기록. 이 검증 전에는 해결로 닫지 않는다.

### D16 · P1 · Sparkle 보안 후속 — 검증 대기

기존 2.9.4는 공식 권고 GHSA-3x7w-j75x-ppq5의 영향 범위 `<=2.9.5`에 포함된다. 시스템 권한 installer에서 경로 검증과 이동 사이의 symlink 교체와 관련된 **로컬·높은 공격 복잡도** 문제이며, Ugurugu의 모든 설치가 공격 가능하다고 단정하지 않는다. [공식 보안 권고](https://github.com/sparkle-project/Sparkle/security/advisories/GHSA-3x7w-j75x-ppq5).

- 구현됨: 같은 2.9 계열의 수정 버전 2.9.6으로 올리고, 공식 배포본에서 직접 계산한 SHA-256 `52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192`를 CMake에 고정했다. [공식 2.9.6 릴리스](https://github.com/sparkle-project/Sparkle/releases/tag/2.9.6).
- 남음: macOS에서 실제 패키지의 framework 버전과 서명, 신규 설치, 2.9.4 포함 기존 앱에서 업데이트, 관리자 권한 경로를 검증한다. delta 비활성화만으로 이 installer 문제를 닫지 않는다.

### D05 · P1 · 최종 저장 이름의 덮어쓰기 확인 — 검증 대기

근거: [SavePathDialog.cpp](../src/ui/SavePathDialog.cpp), [MainWindow.cpp](../src/ui/MainWindow.cpp)의 저장, [MainWindowExport.cpp](../src/ui/MainWindowExport.cpp), [MainWindowSettings.cpp](../src/ui/MainWindowSettings.cpp).

기존에는 대화상자가 반환한 이름을 확인한 **뒤** 확장자를 정규화했다. 함수는 접미사가 없을 때뿐 아니라 기대 접미사와 다를 때도 확장자를 덧붙였으므로, 대화상자가 확장자를 자동 보정하지 않는 경로에서는 확인 대상과 실제 교체 파일이 달라질 수 있었다. 모든 OS 기본 대화상자에서 재현됐다는 뜻은 아니다.

- 구현됨: 공용 저장 대화상자가 형식별 기본 접미사를 선택 전에 설정한다. 알려진 이미지 접미사는 선택 필터보다 우선해 PNG/JPEG 명시를 보존하고, 알 수 없는 접미사는 선택 형식의 접미사를 덧붙인다. 정규화로 경로가 바뀌고 그 최종 대상이 이미 있으면 별도 확인한다. 프로젝트·PNG/JPEG·GIF/WebP·WWP 프리셋이 같은 경로를 사용하며 기존 `QSaveFile` 저장은 유지한다. [Qt `defaultSuffix`](https://doc.qt.io/qt-6/qfiledialog.html#defaultSuffix-prop), [Qt overwrite 확인 기본값](https://doc.qt.io/qt-6/qfiledialog.html#Option-enum).
- 회귀 추가: 기본 접미사, 확장자 없음·다른 접미사·대소문자가 다른 같은 접미사, 정규화 뒤 확인 취소 시 기존 파일 보존, 이미 확인된 같은 이름 경로를 검사한다.
- 확인 (2026-10-03, Windows 11 x64, Qt 6.11.2 Release(ClangCL)): offscreen(Qt 위젯 대화상자)에서 `configuresTheSaveDialogDefaultSuffix`·`normalizesSaveExtensions`·`cancelingFinalOverwritePreservesTheExistingFile`(2행)·`acceptsAnAlreadyConfirmedFinalSavePath` 통과. windows 플랫폼(Windows 기본 대화상자)에서는 나머지는 통과하지만 `configuresTheSaveDialogDefaultSuffix`가 간헐적으로 실패한다(이날 9회 중 5회, `reject()` 뒤에도 경로가 반환됨). 같은 날의 다른 변경(D02) 전후 모두에서 나타났다. 테스트가 기본 대화상자가 뜨기 전에 `reject()`를 부르는 시점 문제인지 제품 동작인지는 확인하지 않았다.
- 원인 확인 (2026-10-05, `295fdd6`): 기본 대화상자에 코드로 `reject()`를 호출하면 `finished(0)` 직후 패널이 `Accepted`로 다시 끝난다. 별도 probe에서 macOS 기본 패널은 지연 0·300ms 모두 매번 그랬다. 제품 코드에는 열린 대화상자를 코드로 닫는 경로가 없으므로 제품 결함이 아니라 테스트 방식 문제다. 테스트는 `AA_DontUseNativeDialogs`로 Qt 위젯 대화상자를 띄워 우리 설정만 검사한다. offscreen·cocoa 모두 통과.
- 남음: Windows·macOS 기본 대화상자에서 접미사 없음·다른 접미사·취소를 사람이 수용 검증한다. 이 검증 전에는 해결로 닫지 않는다.

### R03 · P1 · 웹 저장 순서 — 부분 해결(자동저장 큐 진입 `58e6aac`, 느린 엔진 매트릭스 미시험)

현재 [App.svelte](../web/src/App.svelte)의 `downloadDocument`는 `enqueueExclusive` 안에서 직렬화한다. 따라서 예전의 **수동 저장이 편집 큐를 우회한다**는 발견은 해당 경로에서 수정됐다. 텍스트 미리보기의 적용/취소 안내도 추가됐다.

2026-10-05 `58e6aac`: 자동저장도 같은 큐(`queued`) 안에서 문서 id·revision·이름·bytes를 함께 캡처한다. 아래 완료 조건의 느린 엔진 매트릭스는 아직 따로 시험하지 않았다. (수정 전) autosave host의 `serialize`는 `engine.serialize()`를 직접 호출했다. 큐에 대기 중인 편집과 복구 snapshot의 순서, 문서 identity·이름·revision의 원자적인 캡처는 남은 과제다. `ready: !drawing`은 대기 중 명령이 없다는 보장이 아니다.

- 완료: 느린 엔진에서 pen-up·undo·레이어 변경 직후 수동/자동 저장, 저장 직후 열기, pending text/transform 각각을 시험한다. 수동 저장 수정은 유지하고 자동저장도 동일한 세션 순서 계약을 따른다.

### R04 · P1 · 웹 자동복구의 문서 세대 — 해결(`58e6aac`, 브라우저 회귀 2026-10-05, main `04aa1d6`)

근거: [AutosaveController.svelte.ts](../web/src/lib/AutosaveController.svelte.ts), [RecoveryStore.ts](../web/src/lib/RecoveryStore.ts). `reset()`은 `savedRevision`만 0으로 바꾸며 진행 중 snapshot을 무효화하지 않는다. 저장 뒤 이전 revision이 새 문서의 저장 상태를 덮을 수 있다. 이름은 serialize 이후에 읽으므로 문서와 이름의 소속도 함께 고정해야 한다.

과거 소스 함수 재현에서는 A의 revision 1 저장을 지연한 뒤 B로 교체하자, B의 revision 1을 저장된 것으로 오인해 snapshot을 생략했다. 저장소는 같은 origin의 단일 `slot`이다.

- 조치: session/project identity·generation·revision을 캡처한다. await 이후 상태 채택뿐 아니라 오래된 IDB write 자체가 현 복구본을 대체하지 못하도록 저장소 키/채택 규칙을 설계한다.
- 완료: serialize 지연, IDB 지연, 연속 문서 교체, 복구 폐기 중 저장 완료, 복수 탭을 시험한다. 낡은 완료가 새 문서의 저장 표시·복구본을 바꾸면 실패다.

### R05 · P1 · 웹 문서 교체 전 미저장 작업 보호 — 해결(`58e6aac`, 브라우저 회귀 2026-10-05, main `04aa1d6`)

근거: [App.svelte](../web/src/App.svelte)의 `createDocument`·`openDocument`·`adoptDocument`. 문서 교체는 큐를 따르지만 수동 저장 기준 dirty 보호가 없다. 정상 파일 열기/새 문서는 이전 handle을 대체한다. 15초 자동저장 간격과 단일 복구 슬롯은 명시적 버리기 동의를 대신하지 못한다.

- 조치: 저장/계속 편집/버리기와 문서별 복구 보존 정책을 정한다. 깨진 파일 열기 실패 시 기존 문서를 유지하는 현재 방어는 보존한다.
- 완료: 첫 자동저장 전 문서 전환, 저장 실패·취소, 복수 탭에서 명시적으로 버리지 않은 작업을 보존한다. 다운로드 시작과 실제 디스크 저장 확인을 같은 의미로 표시하지 않는다.

### R06 · P2 · 네이티브 미적용 텍스트의 소속·저장 경계 — 미해결

근거: [CanvasWidgetText.cpp](../src/ui/CanvasWidgetText.cpp), [CanvasWidget.cpp](../src/ui/CanvasWidget.cpp)의 문서 교체, [MainWindow.cpp](../src/ui/MainWindow.cpp)의 `hasUnsavedWork`·저장. 텍스트 배치가 dirty 판단/저장에 포함되지 않고 문서 교체 뒤에도 남을 수 있다. 과거 실제 UI 함수 probe에서는 미리보기 상태로 저장한 파일은 획 0개였고, 새 문서에서 적용하면 22개 획이 생겼다. 전체 메뉴 클릭을 재현한 테스트와는 구분한다.

- 조치: 텍스트·부동 변환·입력 중 획의 적용/취소 정책을 저장·내보내기·열기·새 문서·닫기에서 공유한다. 도구 문자열 재사용과 이전 문서의 미적용 배치는 분리한다.
- 완료: A의 pending edit가 안내 없이 저장에서 빠지거나 B에 적용되지 않는다. 다중 glyph의 부분 커밋은 기존 `MacroTransaction` 방어가 있으므로 별도 발견으로 세지 않는다.

### D07 · P1 · 트랜잭션 결과와 no-op 계약 — 미해결

근거: [DocumentController.cpp](../src/document/DocumentController.cpp)의 `endHistoryMacro`·setter, [DocumentUndoStack.cpp](../src/document/history/DocumentUndoStack.cpp), [LayerDock.cpp](../src/ui/LayerDock.cpp).

실패 매크로는 staged 효과를 폐기하지만 호출자에게 결과를 반환하지 않는다. 같은 값 설정을 실패로 취급하는 setter와 그렇지 않은 경로도 혼재한다. **실패 후 선택만 부분 커밋된다는 예전 추정은 채택하지 않는다.** 트랜잭션 폐기와 UI 결과 전달 누락은 다른 문제다.

- 조치: committed/unchanged/rejected 계약, no-op의 매크로 의미, undo/redo preflight 거부를 정의한다. 모든 void API 변경은 별도 범위 판단 후 진행한다.
- 완료: no-op을 포함한 성공 매크로, 중간 거부, 예산 초과, 선택+resize 실패에서 문서·선택·히스토리·UI 안내를 함께 검사한다.

### D19 · P2 · 네이티브 복구본의 출처 표시 — 미해결

근거: [MainWindow.cpp](../src/ui/MainWindow.cpp)의 `recoverAutosave`, [RecoveryWriter.cpp](../src/app/RecoveryWriter.cpp). 원본 경로 메타데이터를 읽지만 복구 후 현재/제안 경로를 비운다. 프롬프트에서 그림의 출처를 알기 어렵다. 메타데이터 포함 직렬화 실패 후 fallback도 진단 대상이다.

- 조치: 복구본 이름·시각·출처와 실패 상태를 표시하고 `원본이름-recovered.ugu` 같은 안전한 제안 이름을 사용한다. 원본 경로로 조용히 재저장하지 않는다.
- 완료: 정상/손상/없는 메타데이터와 fallback, 저장 취소를 시험하고 원본 파일을 보존한다.

## 4. 렌더·변환·입력·접근성

### D01 / R02 · P1 · 이미지와 선택 변환의 합성 순서 — 해결

근거: Qt는 `QTransform` 곱의 왼쪽 변환을 먼저 적용한다. [Qt 변환 합성](https://doc.qt.io/qt-6/qtransform.html#combining-transforms). [DocumentControllerStrokes.cpp](../src/document/DocumentControllerStrokes.cpp)의 마스크 없는 `duplicateStrokes`·`transformStrokes`는 자산→문서 배치 뒤에 문서 좌표 delta를 적용하도록 `existing * delta`로 결합한다. [CanvasWidget.cpp](../src/ui/CanvasWidget.cpp)와 [CanvasWidgetSelection.cpp](../src/ui/CanvasWidgetSelection.cpp)의 떠 있는 선택 세션도 기존 누적 변환 뒤에 새 이동·회전·크기·뒤집기 delta를 붙인다. 마스크 분기의 직렬화된 `PixelSelectionOp` 계약은 바꾸지 않았다.

- 회귀: [StrokeCommandTests.cpp](../tests/StrokeCommandTests.cpp)는 비균일 축소·중앙 배치 이미지의 복제와 이동→회전→뒤집기를 독립적인 점 매핑 oracle, 렌더 픽셀, undo/redo와 비교한다. [UiSelectionTests.cpp](../tests/UiSelectionTests.cpp)는 떠 있는 선택 영역에서 같은 연속 동작의 누적 행렬, 미리보기·커밋 픽셀, undo/redo를 비교한다.
- 확인 (2026-10-03, Windows 11 x64, Qt 6.11.2 Release(ClangCL)): `composesPlacedImageTransformsInDocumentCoordinates`(document 스위트)와 `composesFloatingSelectionActionsInDocumentOrder`(ui_selection, offscreen·windows 플랫폼 모두) 통과. 수정 전 코드에서 실패하는지는 다시 확인하지 않았다.
- 확인 (2026-10-05, macOS 27 arm64, Qt 6.11.2): `document` 스위트(offscreen)와 `ui_selection` 스위트(offscreen·cocoa) 통과.

### D02 · P1 · 우글거림 OFF에서 pen-up 프리뷰 승격 — 해결 (`bc36217`)

근거: [CanvasWidgetTools.cpp](../src/ui/CanvasWidgetTools.cpp)의 `endStroke`는 `activeStrokePreview`에 원본 controller 문서를 넘겼고, 다른 표시 경로는 문서·레이어 wobble을 제거한 `displayDocument()`를 넘겼다. 우글거림을 끈 상태에서 승격된 프레임은 우글거린 획으로 그려진 채 정확한 프레임으로 캐시에 들어갔다.

- 수정: `activeStrokePreview`가 문서를 인자로 받지 않고 항상 `displayDocument()`로 합성한다. 호출자가 다른 문서를 넘길 경로가 없다.
- 회귀: [UiViewportTests.cpp](../tests/UiViewportTests.cpp)의 `promotesThePenUpFrameOfTheDisplayedDocument` — 문서·레이어 wobble × ON/OFF에서 승격 캐시 프레임과 표시 프레임이 `renderScaled(displayDocument(), …)`와 같고, 재생을 켰다 끈 뒤에도 같다. 수정 전에는 OFF 두 행이 실패하고 ON 두 행은 통과했다. offscreen CTest 13/13, windows 플랫폼에서도 통과.

### B-01 · P1 · 클리핑 레이어가 얹힌 레이어의 아래 병합 — 해결 (`c247645`)

근거: 클립 레이어는 바로 아래의 비클립 형제를 기준으로 잘린다. [DocumentControllerLayers.cpp](../src/document/DocumentControllerLayers.cpp)의 `mergeLayerDownStatus`는 원본 바로 위 형제가 클립 레이어여도 병합을 허용했고, 병합 뒤 클립 기준에 아래 레이어 그림이 더해져 픽셀이 바뀌었다([ANALYSIS_REPORT.md](ANALYSIS_REPORT.md) B-01).

- 수정: 원본 바로 위 형제(컴포지션 계획과 같은 순서)가 클립 레이어이면 `UnsupportedProperties`를 반환한다. 병합 버튼 툴팁과 ko/ja 번역에 이 조건을 넣었다.
- 회귀: [LayerCommandTests.cpp](../tests/LayerCommandTests.cpp)의 `refusesToMergeALayerThatAClippingLayerRestsOn` — 병합 거부·문서와 픽셀·undo 스택 불변, 사이에 일반 레이어를 두면 병합이 허용되고 픽셀이 같다. 수정 전 실패.

### B-02 · P1 · 렌더·디코드 예외 뒤 캐시 슬롯 영구 대기 — 해결 (`00eac5d`)

근거: [StaticLayerCache.cpp](../src/render/engine/StaticLayerCache.cpp)와 [RasterAssetCache.cpp](../src/render/RasterAssetCache.cpp)는 렌더·디코드가 정상 반환할 때만 계산 중 표시를 지웠다. 한 번 `bad_alloc`이 나면 같은 레이어·자산을 요청하는 이후 렌더가 모두 대기했고, GUI 스레드의 동기 렌더면 화면이 멈췄다([ANALYSIS_REPORT.md](ANALYSIS_REPORT.md) B-02).

- 수정: 렌더를 맡은 스레드가 `PendingRender`(RAII)와 scope guard로 슬롯을 쥐고 모든 종료 경로에서 정리한 뒤 대기자를 깨운다. 자산 캐시의 계산 중 표시 로직은 단독 시험이 가능하도록 [ComputedImageCache](../src/render/engine/ComputedImageCache.hpp)로 분리했다.
- 회귀: [WobbleAnimationTests.cpp](../tests/WobbleAnimationTests.cpp)의 `releasesAStaticLayerSlotWhoseRenderThrew`, [ComputedImageCacheTests.cpp](../tests/ComputedImageCacheTests.cpp)(동시 미스 1회 계산, 예외 뒤 대기자 재계산). 수정 전 실패. 2026-10-03 Windows 11 x64, Qt 6.11.2 Release(ClangCL) offscreen CTest 13/13, PR #10 CI(macOS ASan+UBSan 포함) 통과.

### D26 · P1 · 2.2.11 회귀: 그리지 않을 때 캔버스 위 브러시 링이 멈춤 — 해결 (Windows·macOS 마우스 확인, 펜 확인 대기)

증상(사용자 보고, 2.2.11): 펜·마우스가 캔버스에 들어온 뒤 버튼을 누르지 않고 움직이면 링이 들어온 자리에 멈춘다. 그리는 중에는 따라가고 캔버스 밖은 정상이다.

원인: 선택 액션바는 GPU 표시 창 위에 보이도록 네이티브 위젯이다(`23bd7a5`). Qt는 네이티브 위젯의 형제를 기본으로 네이티브로 만들기 때문에, 형제인 window container가 자체 네이티브 창을 갖고 캔버스와 표시 창 사이에 놓였다. 표시 창은 `WindowTransparentForInput`이라 OS가 아래 창에 입력을 넘기는데, 그 창이 container였다. container의 `WA_TransparentForMouseEvents`는 같은 창 안에서 위젯을 고를 때만 쓰이므로, 버튼 없는 이동을 container가 받고 버렸다. 버튼을 누르면 grab으로 이벤트가 캔버스에 바로 가서 그리는 중에는 정상이었다.

- 확인 (2026-10-05, Windows 11, 실행 중인 2.2.11과 같은 코드의 Release 빌드, `SendInput` 마우스): 앱 전역 이벤트 계측에서 hover `MouseMove` 98건이 모두 `QWindowContainer`로 가고 `CanvasWidget`에는 0건이었다. 표시 창은 요청이 올 때마다 정상적으로 present했고, 링 갱신 요청은 입장 때 1번뿐이었다.
- 수정: container가 네이티브 창을 갖는 순간(`WinIdChange`) 그 창도 입력 투명으로 만든다. 디스플레이 스택의 네이티브 창은 모두 입력을 통과시킨다는 불변식이며, Qt가 왜 네이티브로 만들었는지와 무관하다.
- 수정 후: hover 이동 98건이 모두 `CanvasWidget`에 도착했고, 실제 화면 캡처에서 링이 커서 위치(x≈1766→2216)를 따라갔다.
- 회귀: [UiViewportTests.cpp](../tests/UiViewportTests.cpp)의 `hoverMovesReachTheCanvasUnderTheGpuDisplay`. 선택 액션바를 붙인 캔버스에서, 스택에서 입력 투명이 아닌 맨 위 창에 버튼 없는 이동을 넣는다. windows 플랫폼에서 수정 전 실패, 수정 후 통과. offscreen에서는 GPU 표시가 없어 건너뛴다. offscreen CTest 12/12, windows 플랫폼 UI 스위트 5개 실패 0.
- 배포: 2.2.12(`caf602c`, 태그 `v2.2.12`). main CI 통과. Release 워크플로 결과와 2.2.11→2.2.12 업데이트는 기록 시점에 확인하지 않았다.
- macOS (사용자 보고, 2026-10-06): 위 수정 뒤에도 cocoa에서는 링이 멈췄다. Qt 6.11.2 `qnsview_mouse.mm`의 `mouseMovedImpl`은 포인터 아래 가장 깊은 자식 QWindow(`childWindowAt`, 입력 투명 여부를 보지 않음)를 `s_windowUnderMouse`로 잡고, 그 창이 아닌 view의 이동과 입력 투명 view의 이동을 모두 버린다. 표시 창이 그 자리라 hover 이동이 어디에도 전달되지 않았다.
- macOS 수정: 표시 창은 macOS에서만 입력 투명을 빼고 포커스 거부만 남긴다. 받은 입력은 기존 `event()` 전달로 캔버스에 간다. macOS 27.0.1 arm64, Homebrew Qt 6.11.2 Release에서 offscreen CTest 12/12, cocoa UI 스위트 5개 실패 0. 위 회귀 테스트는 QWindow에 직접 이벤트를 넣어 NSView 단계의 누락을 재현하지 못한다. 사용자가 macOS 실기에서 마우스 hover 때 링이 따라오는 것을 확인했다.
- 남음: 실제 펜(WinTab) hover, macOS 펜 hover와 표시 창을 거치게 된 클릭·휠·제스처 확인.

### D03 · P1 · 포커스·근접 이탈 시 획 처리 — 미해결 / 정책 필요

근거: [CanvasWidgetEvents.cpp](../src/ui/CanvasWidgetEvents.cpp)와 [MainWindow.cpp](../src/ui/MainWindow.cpp)는 FocusOut·UngrabMouse·TabletLeaveProximity·WindowDeactivate에서 취소 경로를 호출하고, [CanvasWidgetTools.cpp](../src/ui/CanvasWidgetTools.cpp)의 `cancelStroke`는 진행 획을 버린다.

- 조치: 일반 포커스 이동, 명시적 취소, 문서 교체, 장치의 실제 cancel을 구분한다. 포커스 상실을 모두 커밋하거나 모든 cancel을 무시하는 방식은 피한다. 이중 처리도 점검한다.
- 완료: release/proximity 순서 변형, 알림·창 전환·문서 교체에서 정의한 대로 정확히 한 번 종료된다. 실제 드라이버가 어떤 순서를 보내는지는 펜 실기기 로그로 확인한다.

### 나머지 정확성·입력 항목

| ID·우선순위·상태 | 현재 근거와 조치 | 완료 기준 |
|---|---|---|
| R07 · P2 · 미해결 | [MainWindow.cpp](../src/ui/MainWindow.cpp)의 wobble 토글은 WebP 액션을 갱신하지 않고 [MainWindowExport.cpp](../src/ui/MainWindowExport.cpp)의 공통 갱신은 GIF/WebP를 함께 처리. 과거 probe에서 상태 불일치 재현. 공통 계산 사용 | ON/OFF/ON × export busy/완료 × 정지 이미지 출력 뒤의 세 형식 상태 |
| D11 · P2 · 조건부 미해결 | [CanvasWidgetEvents.cpp](../src/ui/CanvasWidgetEvents.cpp)의 비좌측 TabletPress는 합성 mouse 팬을 시작할 수 있으나 TabletMove는 active tablet sequence가 아니면 소비. 가운데 버튼 매핑 기기에서 확인 필요 | mouse/tablet 중복 없이 press/move/release·포커스 이탈 팬 동작 |
| D12 · P2 · 미해결 | [MainWindowActions.cpp](../src/ui/MainWindowActions.cpp)의 window 범위 Return 변환 적용이 편집기 Enter와 충돌할 가능성. 캔버스/세션 맥락으로 제한 | pending transform 상태의 line edit·spinbox·IME·버튼 Enter 보존 |
| D13 · P2 · UX 결정 | [MainWindow.cpp](../src/ui/MainWindow.cpp)의 회전은 즉시 적용, 크기 변경은 pending. 즉시/대기 정책과 안내를 통일할지 결정 | 적용·취소·undo·연속 회전/크기 변경이 선택한 정책과 일치 |
| D20 · P2 · 미해결 | [DocumentControllerStrokes.cpp](../src/document/DocumentControllerStrokes.cpp)의 일반 변환은 구형 `visibilityClip`을 옮기지 않음. 복제 경로는 실체화. 구형 reader의 클립 보존 사례 필요 | schema 5 이전 fixture의 이동/회전/복제·재저장 픽셀 검증. Android reader 제거로 데스크톱 문제를 닫지 않음 |
| R08 · P2 · 부분 해결 | [Shortcuts.ts](../web/src/lib/Shortcuts.ts)의 range 방향키·button Enter가 앱 명령으로 처리됨. 과거 소스 함수 재현. 2026-10-05 코드 기준 button Enter/Space는 App에서 보호됨. range·checkbox의 방향키·Enter·Delete는 여전히 앱 명령(`isTextEntry`) | Tab으로 range/Home/End/방향키, button Enter/Space를 기본 의미로 사용 |
| R09 · P2 · 해결(`13d1b56`, 브라우저 회귀 2026-10-05, main `2f33138`) | (수정 전) [App.svelte](../web/src/App.svelte)의 `animateWhileDrawing` 초기화가 localStorage를 보호 없이 읽음. 저장소 접근이 네 호출부에 흩어져 각자 try/catch하던 구조에서 한 곳이 빠졌고, 사이트 데이터 차단 시 getter 예외로 셸 전체가 시작하지 못했다. 이제 모든 preference 접근이 [Preferences.ts](../web/src/lib/Preferences.ts)를 거치고 거부는 기본값으로 처리 | getter/getItem/setItem/IDB 각각 실패해도 새 문서·다운로드 가능 |
| R10 · P2 · 미해결 | 웹 dialog·sheet의 focus 순환/복귀·배경 단축키 차단 불일치. `aria-modal`과 실제 배경 조작 정책을 맞춤 | Tab/Shift+Tab/Escape/호출 버튼 복귀, 크기 dialog 뒤 문서가 단축키로 바뀌지 않음 |
| R12 · P2 · 미해결 | [ColorWheel.cpp](../src/ui/ColorWheel.cpp)·[ColorDock.cpp](../src/ui/ColorDock.cpp): ClickFocus·마우스 중심, HEX는 표시용. 검증 가능한 HEX/RGB 입력과 교환 액션 필요 | 키보드로 임의 색 입력·실제 브러시 적용·값 낭독. accessibleName 하나로 전체 접근성 판단 금지 |
| R13 · P3 · 미해결 | [Theme.cpp](../src/ui/Theme.cpp)의 비선형 RGB 밝기 선택. 과거 `#00B900`에서 밝은 후보 약 2.54:1 / 어두운 후보 약 6.70:1 계산 | sRGB 상대 휘도 대비로 후보 선택, hover/pressed·명암 테마 확인 |
| R14 · P2 · 개선 제안 | [ColorHistoryGrid.cpp](../src/ui/ColorHistoryGrid.cpp)의 빈 256슬롯과 기본 dock 배치가 도구 설정/레이어보다 공간 우선. 과거 1440×900·1024×768 offscreen 관찰 | 빈 기록 축소·주요 설정 노출을 실제 DPI/OS에서 검증. 기존 사용자 배치 보존 |

### D17 · P2 · 조작 일관성과 도움말 — 혼합 항목

- 현재 단축키 변경과 도움말의 하드코딩된 힌트가 어긋나지 않도록 액션에서 설명을 생성한다. `^`의 ANSI/JIS 차이와 수정자 기반 입력 충돌도 확인한다.
- Windows 설정 열기, 프레임 이동의 전역 접근, 레이어 이름 변경/F2·메뉴·키보드 조작은 제품 선택으로 검토한다. scrubber의 기존 방향키 지원을 “단축키 전무”로 기술하지 않는다.
- 휠의 확대/이동, 수평 휠, 캔버스 바깥에서 시작하는 획, double-click 점, Alt 충돌은 **UX 결정 또는 재현 대기**다. double-click 이벤트 기본 전달을 확인하기 전 두 번째 점 누락을 확정하지 않는다. 바깥 점을 무조건 clamp하면 가장자리 획이 생길 수 있다.
- Copy가 새 레이어도 만드는 것은 기존 테스트로 고정된 동작이다. 결함으로 단정하지 않고 명칭/설명 또는 Copy와 Duplicate 분리를 검토한다.
- 그룹 선택 중 그리기 거부, 문자 한 글자 레이어 배지, 현재 도구·적용 대상·pending 상태의 발견성을 개선한다. 신규 사용자 과업 테스트 없이 사용성 향상 수치를 주장하지 않는다.

완료 기준은 키보드·펜·마우스·트랙패드별 핵심 과업, 큰 글씨·고배율·한/영/일·스크린리더 확인이다. 자유형 그리기의 키보드 대체와 주변 UI 조작 가능성은 구분한다.

## 5. 메모리·성능: 구현된 것과 측정할 것

### 기존 성능 후속 작업

| 항목 | 상태 | 남은 검증 |
|---|---|---|
| 정지 중 현재 프레임만 준비 (`05276c8`) | 구현·조건부 A/B 기록 있음 | 다른 플랫폼·입력·표시 조건으로 일반화 금지 |
| 성공 stroke 승격 뒤 중복 interaction warmup 제거 (`9ce634c`) | 구현·회귀 테스트 있음 | Release 50회 이상 A/B, worker·픽셀·fallback |
| RenderEngine 내부 cancellation 전파 (`fe51f80`) | 구현·회귀 테스트 있음 | 취소 뒤 잔여 CPU·중단 지연·정상 렌더 A/B |
| regional patch 완료의 UI 전체 프레임 복사 | 조사 | UI callback p50/p95/max·복사량·이벤트 대기 |
| 백그라운드 작업 우선순위·동시성 | 조사, D08과 통합 | 입력·재생·복구 경쟁, peak RSS·동시 worker |

역사적 `05276c8` 측정은 macOS Cocoa Release / software / 회전 0도 / wobble ON / 재생 정지 / 같은 사용자 문서로 변경 전후 각 24회다. 획당 process CPU p50 258.64→126.05ms, 최장 live-display event p95 12.10→7.40ms였다. 획 전체 wall p95 92.81→91.92ms는 유의미한 개선 근거가 없고 pen-up p50 0.79→0.84ms도 개선으로 보고하지 않는다. 이후 두 변경의 효과와 합산하지 않는다.

### 2026-10-03 데스크톱 렌더·UI 경로 개선 — 구현·Windows 측정

브랜치 `perf/desktop-render-pipeline`, 기준 `be2a06d`. 측정 조건은 위 증거 범위 표와 같다. 렌더 수치는 `ugurugu_render_benchmark`(문서 전체 프레임, 프레임당 시간·SHA-256 digest)로, UI 수치는 임시 probe로 수정 전후 같은 빌드 설정에서 쟀다. 표본 문서는 1024×768·7레이어·252획 사용자 문서(저장소 미포함)와 `ugurugu_stress_document_generator` 2048² 문서다. 모든 렌더 A/B의 digest가 수정 전과 같다.

| 항목 | 구현 (commit) | 측정 |
|---|---|---|
| 정적(프레임 불변) 레이어 래스터 캐시, 빈 레이어 표면 생략 | 획 목록·래스터 자산의 implicit sharing으로 재사용을 판정하는 프로세스 공용 캐시, 256MiB 예산. 모든 전체 레이어 렌더 경로가 `renderPaintLayerImage` 하나를 거친다 (`37dd9f1`) | 사용자 문서 768×576: 정지 레이어 0개 60→60ms/프레임(변화 없음), 3개 고정 시 60→15ms. stress 50획/레이어 1024²: 2/4 레이어 고정 시 preview 340→165ms, NativeExact 495→250ms. 빈 레이어 4개 추가 시 표면 할당 270→150 |
| 무효화 뒤 GUI 동기 렌더와 워커 중복 | 워커가 같은 프레임을 전달할 예정이면 화면은 직전 프레임을 유지하고 전달 시 다시 그린다. 문서 교체 시에는 유지하지 않는다 (`7001a52`) | 정지·재생 모두 편집당 GUI 동기 렌더 1→0회. 사용자 문서 기준 GUI thread에서 약 60ms 렌더 1회가 빠진다 |
| 재생 중 펜 다운의 GUI split 렌더, 분할 불가 레이어의 프레임 캐시 전체 폐기 | 워커가 준비할 수 있으면 펜 다운도 워커 split을 기다리고, 워커가 예산 안에서 못 만들면 한 번만 GUI가 만든다. GUI 경로는 추정 래스터 크기만큼만 캐시를 줄인다 (`9d83547`) | 펜 다운 GUI 렌더 split 1→0, raster 2→0. 캐시 4프레임 상태에서 펜 다운 뒤 남는 프레임 1→4 |
| `previewRenderSize()` 합성 계획 2회 빌드 | 입력이 같으면 결과 재사용 (`e0a494d`) | 호출당 약 2.5µs→0.035µs(사용자 문서) |
| 색 기록 256버튼 stylesheet | 직접 그리는 swatch, 바뀐 항목만 repaint (`5b2a854`) | 색 1개 기록+paint 13–16ms→2.3ms |
| 팬·회전마다 그림자 재생성 | 윤곽 주변만 덮고 원점 기준 윤곽으로 키를 잡는 pixmap (`43a1da5`) | software 표시 1400×1000 팬+repaint 27–30ms→2.7–3.0ms. 회전·확대 시에는 여전히 다시 만든다 |
| 포인터 이동마다 GPU frame view 렌더·상태바 relayout | 커서·윤곽 변경은 overlay만 갱신, 좌표 라벨 고정 크기·같은 문자열 생략 (`5001d29`) | D3D11 창 hover 이동 300회: frame view `render()` 305→4회 |
| 스포이드·마술봉·페인트통의 원본 해상도 GUI 렌더 | 해당 도구가 활성일 때 워커가 참조 이미지를 미리 렌더하고 정확한 입력이 같을 때만 쓴다. 진행 중이면 기다리고, 없을 때만 GUI가 렌더한다 (`58f43ff`) | 사용자 문서: 스포이드 첫 pick 67–76ms→0.1–0.2ms, 페인트통 클릭(활성 레이어 기준) 10ms→7ms |
| 타임라인 스핀박스 키 입력마다 커밋 | 프레임 수·FPS는 편집 완료 시 커밋 (`b0d55be`) | "24" 입력당 문서 변경 2→1회 |
| RasterAssetCache 동시 미스 | 키별 in-flight 표시로 첫 스레드만 계산 (`346209e`) | 2048² 자산 1개, 8프레임 동시 cold 렌더: process CPU 2.36s→0.29s, peak working set 369→145MiB, wall 약 300ms 동일 |
| 필압 획의 증분 체크포인트 | 가변 필압 선은 점마다 독립 합성되는 조각의 연속이므로, 타일이 전체 획과 같은 조각만 그리게 해 조각 경계에서 체크포인트를 자른다. AA·반투명·지우개 포함 픽셀 동일. 체크포인트는 이번 갱신이 그린 타일과 최근 16개 타일만 유지 (`1a789b0`) | 한 타일 안 1,500점 획: 가변 필압 469→126ms, primitive 240,477→61,101. 상수 필압 91–96ms로 변화 없음. 4K 50,000점 가변 필압: 40.2s→6.0s, 갱신 p50 0.72→0.07ms. 체크포인트 상한은 레이어 1장+그린 타일+16타일 |
| `.ugu` 저장 (UI 동기) | 저장·다른 이름 저장은 문서 snapshot을 전용 스레드에서 쓰고 편집은 계속된다. snapshot 이후 편집이 없을 때만 저장됨으로 표시하고 복구본을 지운다. 닫기·열기·새 문서·미저장 확인은 진행 중 저장을 기다린다 (`50c7a2a`) | stress 2048²(11.9MB) Ctrl+S의 GUI 점유 157ms→0.4ms. 동기 저장 경로는 157ms로 동일 |
| 내보내기의 전 프레임 버퍼 | 인코더가 프레임을 요청할 때 렌더. WebP는 즉시 인코딩, GIF는 두 패스 사이에 픽셀당 2바이트 키만 보존 (`f57dd94`) | 사용자 문서 30프레임: GIF peak working set 190→65MiB, WebP 131→44MiB. 시간 GIF 2.3s·WebP 6.5–6.7s로 차이 없음. 출력 파일 바이트 동일 |

측정했지만 이번에 바꾸지 않은 것:

- **`.ugu` 열기 (UI 동기 유지)**: 사용자 문서 읽기 11ms·문서 채택 11ms, stress 2048² 112·128ms. 열기는 미저장 확인 뒤 현재 문서를 교체하는 작업이라 진행 중 편집을 받을 수 없고, 채택은 GUI 스레드에서 해야 한다. 비동기로 바꿔도 입력을 막아야 하므로 얻는 것은 약 110ms 동안의 repaint뿐이어서 바꾸지 않았다.
- **재생 중 텍스처 전체 업로드**: D3D11 1400×1000 창 재생에서 프레임당 약 3MiB, frame view `render()` 평균 0.63ms. GPU 쪽 프레임 텍스처 캐시는 VRAM을 프레임 수만큼 쓰므로 이득이 측정될 조건(큰 미리보기·저사양 GPU)을 먼저 찾는다.
- **데스크톱 LTO**: ClangCL 툴셋은 CMake IPO 속성을 무시하므로 `-flto=thin`을 직접 줘서 bitcode 빌드를 확인했다. 위 세 렌더 workload 모두 ±1% 이내로 차이가 없어 넣지 않았다. 렌더 시간 대부분이 LTO 대상이 아닌 Qt 래스터 엔진 안에 있다.

남은 검증: macOS Metal·Apple Silicon 측정, 실제 펜 입력에서 펜 다운 지연, 정적 레이어 캐시와 참조 이미지 선렌더의 장시간 메모리 상한(예산 256MiB와 이미지 1장)을 실제 작업 문서로 확인.

### 그리는 중 입력 지연 — 원인 3건 수정, 실제 펜 확인 대기

**정정:** 이전 기록의 "이동당 6.5ms, 400점 획 약 2.6s"는 probe가 이벤트마다 `processEvents`로 화면 갱신을 강제해 생긴 값이다. 실제 입력처럼 별도 스레드가 `QWindowSystemInterface`로 태블릿 이동을 4ms 간격(240Hz)에 넣고 이벤트 루프를 그대로 돌리면, 수정 전에도 GPU·software 모두 획이 약 1.6s에 끝났다. 지연은 처리량이 아니라 **프레임당 비용이 창 넓이에 비례한 것**에서 나왔다.

측정 조건: Windows 11, 160Hz 모니터·배율 150%, 사용자 문서, 필압 변화 400점, 실제 D3D11 창(작은 창 1400×1000 / 큰 창 2400×1300 논리 픽셀), 정지·재생·재생+그리는 중 애니메이션 각각. 같은 probe로 수정 전(`bbb097b`)과 후를 쟀다. "입력→프레임"은 이동 주입부터 그 점을 포함한 프레임의 Qt flush 반환까지이며, 화면에 실제로 보이기까지의 시간은 아니다.

| 조건 (정지) | 프레임당 GUI 처리 | 프레임당 래스터 repaint | 입력 대기 p50 | 입력→프레임 p50 / p95 |
|---|---|---|---|---|
| GPU 작은 창 | 5.40 → 5.15ms | 1.41 → 0 Mpx | 2.5 → 2.1ms | 8.6 / 12.4 → 8.3 / 11.1ms |
| GPU 큰 창 | 13.0 → 5.1ms (1.6s 동안 113 → 258프레임) | 3.15 → 0 Mpx | 7.2 → 2.2ms | 20.1 / 27.7 → 8.3 / 11.2ms |
| software 큰 창 | 13.3 → 1.3ms | 3.14 → 0.24 Mpx | 7.0 → 0.0ms | — |

재생·재생+그리는 중 애니메이션도 GPU는 같은 값(프레임 5.1–5.2ms, 입력→프레임 p50 8.3–8.4ms)이다. GPU 팬(가운데 버튼 250Hz 드래그 1.2s)은 큰 창에서 프레임당 12.1 → 6.1ms(99 → 194프레임), 작은 창은 6.1ms로 변화 없다.

- **원인 1 · GPU (`72087a6`):** 투명 overlay 위젯이 QRhiWidget frame view 위에 겹쳐 있어, frame view를 갱신할 때마다 Qt가 overlay와 그 아래 캔버스를 화면 전체 넓이로 다시 그리고 backing store를 다시 올렸다. overlay 위젯을 숨기면 이 비용이 사라지고 프레임이 창 크기와 무관해지는 것을 먼저 확인했다. 수정 후 overlay는 frame view가 자체 이미지에 그려 dirty 영역만 다시 그리고 올리며, 같은 pass에서 premultiplied blend로 합성한다. 획 미리보기·재생·워커 프레임 전달처럼 프레임 픽셀만 바뀌는 갱신은 overlay를 무효화하지 않는다.
- **원인 2 · software (`77a9c7c`):** paint 사이에 들어온 두 번째 이후 이동은 변경 영역을 모른다는 이유로 viewport 전체를 다시 칠했다. 창이 클수록 paint가 느려져 paint 사이 이동이 늘고, 그래서 거의 모든 paint가 전체 paint가 됐다. 이제 그 이동들은 한 번의 큐된 resolve로 바뀐 타일만 다시 칠한다. paint 뒤 첫 이동의 즉시 resolve는 유지했다.
- **회귀:** [UiViewportTests.cpp](../tests/UiViewportTests.cpp)의 `drawsWithoutRepaintingRasterWidgetsOverTheGpuDisplay`(수정 전 이동마다 캔버스 paint 21회 → 0, 원인 3에서 아래 테스트로 대체), `gpuDisplayMatchesTheSoftwareDisplay`(GPU와 software 출력의 채널 차이 8 이하, 수정 전후 모두 통과), `repaintsOnlyTheStrokeTailForReportsBetweenPaints`(수정 전 실패). GPU 테스트는 headless에서 건너뛰므로 `QT_QPA_PLATFORM=windows`로 확인했다. offscreen CTest 13/13. windows 플랫폼에서 실패하는 기존 UI 테스트 6개는 수정 전에도 같게 실패한다.
- **표시 차이:** overlay를 별도 레이어로 합성하므로 테두리·커서 링의 안티앨리어싱 가장자리가 software와 최대 5단계 다르다. 빈 문서 안내 문구는 GPU에서 회색조, software에서 ClearType으로 그려진다.

#### 원인 3 · 실제 앱에서의 QRhiWidget 합성 (`23bd7a5`)

위 측정은 캔버스 위젯만 띄운 것이었다. MainWindow 전체를 띄우고 같은 방식으로 8초(2,000점) 획을 넣자 프레임이 vsync 두 번(약 11.4ms)으로 늘고 초당 약 80프레임이 됐다. 그리는 동안 상태바 좌표 라벨(이동마다), 브러시 프리셋 미리보기 애니메이션(약 40Hz), 프리셋 패널(약 8Hz), 재생 중 타임라인이 다시 칠해졌고, 그때마다 캔버스 위젯 전체(1956×1148)가 함께 다시 칠해졌다. 상태바와 프리셋 패널을 숨기면 프레임이 160Hz(1,280프레임, p95 5.3ms)로 돌아왔다.

Qt 6.11.2 `QWidgetRepaintManager::paintAndFlush`는 래스터로 칠할 영역이 하나라도 있으면 갱신된 render-to-texture 위젯의 사각형 전체를 래스터 dirty 영역에 더한다. 즉 QRhiWidget은 창 안 다른 위젯이 칠해지는 프레임마다 아래 영역 전체를 다시 칠하고 업로드한다. QRhiWidget을 네이티브 창으로 만드는 실험은 더 나빴다(1.6초 동안 396프레임).

- **수정:** 캔버스 표시를 `CanvasDisplayWindow`(자체 QRhi swap chain을 가진 `QWindow`, `createWindowContainer`로 삽입)로 바꿨다. 다른 위젯과 캔버스가 서로의 합성에 끼어들지 않는다. 창은 마우스 입력에 투명하고(`WindowTransparentForInput`, 포커스 없음), 플랫폼이 그래도 창에 보내는 펜·터치·제스처·Enter/Leave는 캔버스 위젯으로 전달한다. 커서 모양은 창에 복사한다. 선택 액션바는 캔버스 위에 보이도록 네이티브 자식으로 만든다.
- **측정 (실제 앱 전체, 2400×1300, 같은 probe, 화면 잠금 상태로 전후 동일 조건):**

| 8초 획 | 수정 전 (`6576695`) | 수정 후 |
|---|---|---|
| 입력→캔버스 프레임 p50 / p95 / max (정지) | 18.7 / 25.2 / 43.1ms | 6.2 / 10.6 / 15.6ms |
| 같은 지표 (재생) | 18.6 / 25.1 / 40.2ms | 6.1 / 10.7 / 14.2ms |
| 입력 대기 p50 / p95 | 6.7 / 11.6ms | 0.4 / 4.0ms |
| 16ms 넘는 프레임 | 6 | 0 |

  "입력→캔버스 프레임"은 이동 주입부터 그 점을 포함한 프레임의 present 반환까지다(수정 전은 창 flush 반환). 화면에 실제로 보이기까지는 아니다.
- **실제 OS 입력 (Windows):** `SendInput` 마우스는 입력 투명 창을 지나 캔버스 위젯에 바로 도착해 획이 그려졌다. `InjectSyntheticPointerInput` 펜(WM_POINTER)은 `CanvasDisplayWindow`가 받아 캔버스로 전달했고 필압 0.30–0.90의 획 2개가 그려졌다. 펜에서 합성 마우스 이벤트는 생기지 않았다. 듀얼 모니터에서는 합성 펜 좌표가 다른 모니터로 매핑되므로 창을 주 모니터에 두고 시험해야 한다.
- **회귀:** `drawsWithoutRepaintingTheCanvasWhileOtherWidgetsRepaint`(캔버스+매번 바뀌는 라벨: 수정 전 이동 20번에 캔버스 paint 20회 → 0), `gpuDisplayMatchesTheSoftwareDisplay`(디스플레이 framebuffer 직접 읽기). `QWidget::grab`은 네이티브 자식 창을 담지 않으므로 화면 픽셀을 읽는 테스트 4개를 `CanvasWidgetTestAccess::grabDisplay`로 바꿨고, 그중 `keepsRegionalStrokePreviewFreeOfSeams`의 배율 1 가정도 고쳤다. offscreen CTest 13/13, windows 플랫폼 UI 스위트는 수정 전과 같은 기존 실패 6개만 남는다.

#### 원인 4 · 획 꼬리 패치마다 프레임 전체 복사 (`c9d6808`)

`7001a52`가 워커 렌더를 기다리는 동안 화면 프레임을 계속 보여 주려고 `m_lastDisplayedFrame`에 표시 프레임을 보관하게 했다. 그리는 중 표시 프레임은 획 꼬리를 제자리에 덧그리는 합성 프리뷰 버퍼(`m_composedPreviewFrame`)이므로, 이 참조 때문에 다음 보고의 `QPainter`가 버퍼를 detach해 매 보고마다 프레임 전체를 복사했다. 비용은 패치 크기가 아니라 렌더 크기에 비례했다.

- **수정:** 합성 프리뷰가 화면에 있는 동안에는 참조 대신 플래그만 두고, 프리뷰를 놓는 모든 경로(`releaseComposedPreviewFrame`)에서 버퍼를 `m_lastDisplayedFrame`으로 옮긴다. 워커 대기 중 표시 동작은 같다.
- **측정 (`continueStroke`+`resolveDisplayedFrame` 1회, 획 400점×3회, 수정 전 → 후 p50 / p95):** 사용자 문서 1024×768 렌더, 캔버스 1956×1148 — software 0.99 / 1.17 → 0.07 / 0.15ms, D3D11 창 0.98 / 1.16 → 0.06 / 0.14ms. 빈 4096² 문서 1704×1704 렌더(D3D11 창) 3.43 / 3.65 → 0.04 / 0.11ms, offscreen 1136×1136 1.60 / 1.81 → 0.05 / 0.13ms. 그리는 동안 남던 프레임 크기 버퍼 1장도 사라진다.
- **실제 앱 전체 (원인 3과 같은 probe: MainWindow 2400×1300, 캔버스 1956×1148, 배율 150%, 160Hz, 2,000점 8초 획, 수정 전 `c9d6e24` / 후 바이너리를 정지·재생 각각 번갈아 3회):** 태블릿 이벤트 처리 평균 0.65–0.67 → 0.15ms, 캔버스 창 프레임 평균 0.51–0.53 → 0.41–0.43ms(그중 resolve 0.20–0.23 → 0.11–0.12ms). 입력→캔버스 프레임 p50 / p95는 3.7–3.9 / 6.8–6.9 → 3.7–3.8 / 6.7–6.8ms로 차이가 없고, 16ms 넘는 프레임은 양쪽 모두 0이다. 이 지연은 GUI 작업이 아니라 present가 다음 vsync(6.25ms 주기)를 기다리는 시간이 대부분이다.
- **이전 기록과의 차이:** 같은 probe로 원인 3 측정 때 기록한 수정 후 값(p50 6.2ms, 프레임당 2.5–2.7ms)은 화면 잠금 상태에서 쟀고, 이번에는 잠금 상태가 아니었다. 이번 수정 전 바이너리도 p50 3.8ms·프레임 0.52ms였으므로 두 기록의 절대값 차이는 측정 조건 차이이며 코드 변경 효과가 아니다.
- **회귀:** [UiViewportTests.cpp](../tests/UiViewportTests.cpp)의 `patchesTheStrokePreviewFrameInPlace`(연속 보고 사이 표시 프레임 버퍼 주소가 같은지, 수정 전 실패). offscreen CTest 13/13, windows 플랫폼 UI 스위트 5개는 software 표시를 전제한 `clipsTheSoftwareCheckerToTheRotatedCanvas` 1건만 실패한다(GPU 창에서는 전제가 성립하지 않음).

#### 인수인계 — 다음 작업

1. **실제 펜 확인(최우선).** 앱은 시작 시 Wacom WinTab을 켜는데, WinTab 패킷은 `QWindowsScreen::windowAt`으로 대상 창을 찾으므로 `CanvasDisplayWindow`로 가서 전달될 것으로 예상하지만 합성 입력으로 재현할 수 없어 확인하지 못했다. 사용자의 펜으로 그리기·필압·지우개 끝·호버 커서 링·근접 이탈을 확인하고, 체감 끊김이 사라졌는지 묻는다. 계측이 필요하면 `QApplication::notify`를 감싸 2ms 이상 이벤트와 태블릿 이벤트를 CSV로 남기는 임시 계측을 다시 만든다(이번 세션의 것은 커밋하지 않았다).
2. **눈으로 확인할 UI.** 선택 액션바가 캔버스 위에 보이는지, 도크를 끌 때 위치 표시가 캔버스에 가려지지 않는지, 창 크기 변경·최소화 복귀·모니터 간 이동(배율 변경)·전체 화면에서 캔버스가 맞게 그려지는지.
3. **미해결 관찰.** windows 플랫폼 테스트에서 획의 첫 이동 직후 캔버스 위젯 전체 paint가 한 번에 2회 생긴다(이후 이동에는 0회). GPU 표시는 유지되고, 활성화·노출·배율 변화 이벤트는 없었다. 원인 미확인이며 회귀 테스트는 첫 이동 뒤부터 센다.
4. **macOS.** 2026-10-05 macOS 27(Apple Silicon, Homebrew Qt 6.11.2)에서 Metal 경로를 빌드했고, cocoa 플랫폼 UI 스위트에서 GPU 표시 테스트(`gpuDisplayMatchesTheSoftwareDisplay` 등)가 통과했다. 실제 앱에서 입력 투명 자식 창의 이벤트 전달, Retina 배율, 트랙패드 제스처 전달은 사람이 확인해야 한다.
5. **남은 지연.** 화면 잠금이 아닌 상태에서 캔버스 창 프레임의 GUI 작업은 평균 약 0.4ms이고, 입력→present 반환 p50 3.8ms의 대부분은 vsync 대기다. 따라서 렌더 스레드로 옮겨도 GUI 작업에서 줄일 여지는 작다. 먼저 PresentMon 등으로 present 이후 화면 표시까지의 실제 지연(스왑체인 대기열 깊이·DWM 합성)을 재고, 그 결과로 frame latency waitable object나 present 직전 최신 획 반영이 필요한지 판단한다. 더 줄여야 한다면 이 구조 위에서 렌더 스레드로 옮기고 present 직전에 최신 획을 반영하는 방법을 검토한다.

probe는 커밋하지 않았다. 재현에는 `kimcozo_service.ugu`(저장소 미포함, 루트에 둠)와 위 조건이 필요하다.

### D27 · P1 · 줌·창 크기 변경 뒤 GUI 스레드 동기 렌더 — 해결 (브랜치 `perf/zoom-background-render`, Windows 측정)

물리 배율 100% 미만에서는 미리보기 렌더 크기가 줌을 따라간다. 줌 입력이 80ms 멈추면 `frameImage()`가 프레임 캐시를 비우고 현재 프레임을 GUI 스레드에서 렌더했다. 줌 경로는 warmup을 예약하지 않았으므로, 재생 중에는 재생 틱마다 프레임 하나씩 GUI 스레드 렌더가 이어졌다. 워커를 기다리는 동안 화면 프레임을 유지하는 경로는 크기가 같을 때만 동작해서 이 경우를 덮지 못했다. 100% 이상에서는 렌더 크기가 원본에 고정되어 문제가 없었다.

- **수정:** 렌더 크기만 바뀐 경우(`previewResizePending`: 캐시 크기는 유효한데 목표 크기가 다름)에는 화면의 프레임을 대역으로 표시한다. GPU 표시는 문서 사각형에 텍스처를 매핑하므로 크기가 달라도 위치가 맞다. 새 크기는 frame-cache warmup이 워커에서 렌더한다. 재생 중이면 전체 프레임, 정지 중이면 현재 프레임만 렌더한다. 대역은 `m_resizeStandInKey`로 식별하고, 문서 내용·크기를 바꾸는 `invalidateFrames`에서 해제한다. 따라서 다른 문서 기하의 프레임이 대역이 되지 않는다.
- **측정 (실제 앱, 4K 모니터 최대화, `kimcozo_service.ugu` 1024×768·30프레임, SendInput 휠 + 2ms 간격 `WM_NULL` 응답 지연, 33ms 넘는 정지 수 / 최대):** 재생 중 빠른 확대 16 / 60ms(1.2초 중 약 0.9초 정지), 느린 확대(150ms 간격) 23–25 / 57ms, 느린 축소 6–7 / 52ms → 모든 시나리오 0 / 최대 5ms. 정지 상태 느린 줌 2–3 / 52ms → 0 / 3.6ms. 정지 상태에서 12칸 축소 후 같은 수만큼 확대해 1.5초 뒤 찍은 화면은 시작 화면과 픽셀이 같다.
- **회귀:** [UiViewportTests.cpp](../tests/UiViewportTests.cpp)의 `rendersTheZoomedPreviewOffTheGuiThread`(paused/playing). 줌 직후 표시가 이전 크기 대역이고, 워커가 새 크기 프레임을 채운 뒤 동기 렌더 수가 늘지 않는지 확인한다. 수정 전에는 두 행 모두 실패한다. offscreen 전체 스위트와 windows 플랫폼 `ui_viewport`·`ui_drawing_tools`가 통과한다.
- **줌 직후 그리기 (실제 앱, 같은 조건):** 휠 4칸 직후 0/40/120/300ms 뒤 획 하나(약 0.4초)를 긋고 되돌리기를 반복했다. 축소·확대를 번갈아 51%↔24% 사이를 오가며, 지연값마다 4회씩, 정지·재생 각각 측정했다. 수정 전에는 줌이 획보다 먼저 정착한 경우(120·300ms) 확대 시행마다 정착 시점에 약 60ms 정지가 1회 있었다. 재생 중 300ms 시행은 약 230ms(정지 4회)였다. 재생 중에는 축소 직후 펜 다운이 23ms까지 걸렸다. 수정 후에는 모든 시행에서 33ms 넘는 정지가 0회이고, 획 도중 최대 6ms다. 획 도중 정지는 수정 전후 모두 없었다. 측정 도구가 화면을 캡처하는 시행에서는 Python GIL 때문에 ping 지연이 약 100ms로 부풀었다. 같은 시점에 앱 전체 이벤트 처리를 계측해 앱 쪽 30ms 초과 작업이 없음을 확인했고, 캡처를 뺀 실행으로 다시 쟀다.
- **남은 것:** 재생 중에는 새 크기 프레임이 도착할 때까지 애니메이션이 대역 프레임에 머문다. 그 시간은 재지 않았다. macOS(Retina·트랙패드 핀치)는 확인하지 않았다.

### D28 · P2 · 재생 상태로 시작할 때 프레임마다 GUI 스레드 렌더 — 해결 (브랜치 `perf/startup-warmup-reschedule`, Windows 측정)

시작 직후 창이 작을 때(캔버스 100×30) 맞춤 줌이 걸리면 warmup이 16×12 크기로 돈다. 창이 최대화되면 첫 표시의 `frameImage()`가 캐시를 새 크기(780×585)로 비우고 현재 프레임을 렌더했다. 이때 이전 크기의 warmup은 그대로 남았다. warmup 결과가 도착하면 크기가 다르다는 이유로 취소만 되었고 다시 예약되지 않았다. 그 뒤 재생 틱마다 나머지 프레임을 하나씩 GUI 스레드에서 렌더했다. 임시 계측으로 이 순서를 확인했다.

- **수정:** `frameImage()`가 캐시를 새 렌더 크기로 비울 때, 이전 크기의 warmup을 취소하고 새 크기로 다시 예약한다. 정지 상태에서는 `scheduleFrameCacheWarmup`의 기존 조건에 따라 아무 일도 하지 않는다.
- **측정 (실제 앱, 벤치 문서를 연 채 시작, 시작 후 6초 `WM_NULL` ping, main `17d95e8` / 수정본 번갈아 3회):** 33ms 넘는 정지 30–32회(합계 1.79–1.90초) → 2–3회(합계 233–272ms). 남은 정지는 GPU 표시 창의 표면 생성(약 200ms, 임시 계측에서 `CanvasDisplayWindow` 이벤트 1건)과, 화면에 아무것도 없을 때의 첫 프레임 렌더 1회다.
- **회귀:** [UiViewportTests.cpp](../tests/UiViewportTests.cpp)의 `warmsPlaybackAtTheSizeTheFirstDisplayRenders`(작은 크기로 warmup을 마친 뒤 창을 키워 표시하고, 새 크기 프레임이 워커에서 채워지는지와 모든 프레임을 넘기는 동안 동기 렌더가 첫 표시 1회뿐인지 확인). 수정 전에는 실패한다. offscreen 전체 스위트와 windows 플랫폼 `ui_drawing_tools`가 통과한다. windows 플랫폼 `ui_viewport`에서는 `defersPreviewRerenderUntilZoomInputIsIdle`이 간헐적으로 실패한다. main 코드에서도 4회 중 1회 실패하므로 이번 변경과 무관한 기존 문제이며, 원인은 확인하지 않았다.

### D08 · P2 · 프레임 warmup의 동시 임시 표면 — 조사·예산 공백

[CanvasWidget.cpp](../src/ui/CanvasWidget.cpp)의 최대 8 worker와 [CanvasWidgetPreview.cpp](../src/ui/CanvasWidgetPreview.cpp)의 동시 렌더에 대해 [PreviewRenderPolicy.cpp](../src/render/PreviewRenderPolicy.cpp)의 임시 비용 계산은 동시 작업 전체를 반영하지 않는다. 보존 표면 예산 테스트가 프로세스 peak를 보장하지 않는다.

조치는 worker별 working set, 고정·보존 표면, 취소 중 작업의 잔여 수명을 포함한 동시성 제한이다. 예전의 “추가 1.5–2.5GiB”는 실제 관측값이 아니므로 수용 기준으로 쓰지 않는다. 정확성·재생 재개 시간과 함께 측정한다.

### D14 · P2 · 스포이드의 전체 동기 렌더 — 부분 해결

[CanvasWidgetTools.cpp](../src/ui/CanvasWidgetTools.cpp)의 참조 이미지는 스포이드·마술봉·페인트통이 활성일 때 워커가 원본 해상도로 미리 렌더한다(`58f43ff`). 축소 preview 픽셀은 쓰지 않는다. 브러시·지우개에서 Alt로 시작한 pick과 떠 있는 선택 변환 중 pick은 여전히 GUI에서 렌더한다. 측정은 5절 표.

### R11 및 웹 메모리 정책 — 미해결

[MemoryPolicy.ts](../web/src/lib/MemoryPolicy.ts)의 신규/resize 최대 변은 desktop 2048·mobile 1024이나 파일 열기는 바이트 크기 중심이며, 공용 reader는 4096까지 허용한다. [App.svelte](../web/src/App.svelte)의 파일 선택은 `arrayBuffer()` 후 크기를 검사한다.

- 읽기 전 `File.size`, 문서 채택 전 크기·decoded raster·mask·렌더/GPU 표면 비용을 검사한다. 큰 파일 허용이 의도라면 별도의 import 정책·축소 제안을 정의한다. 자동 축소 후 원본 덮어쓰기는 금지한다.
- WASM 최대 heap 512MB는 JS·GPU·브라우저 전체 예산이 아니다. CPU 표면과 GPU texture 중복·export·serialize의 동시 peak를 측정한다.
- **정정:** 공용 `DocumentUndoStack`에는 현재 192MiB resident 기준과 byte 기반 정리가 있다. 다만 가장 최근 항목 하나는 soft exceed가 가능하다. 웹 `ugu_set_undo_limit`은 개수만 설정하므로 웹 프로파일의 MiB 정책을 연결/관측하는 작업이 남는다. “엔진에 byte 예산 없음”은 코드 주석에도 남은 오래된 설명이다.

### D21 · P3 · UI 갱신 비용 — 부분 해결

색 기록 stylesheet(`5b2a854`)와 타임라인 숫자 입력 도중 커밋(`b0d55be`)은 해결했다. [Theme.cpp](../src/ui/Theme.cpp)의 강조색 변경 시 폰트·스타일 재등록은 남았다. 일회성 초기화 분리, 변경된 항목만 갱신, 숫자 편집 완료 시 적용을 검토한다. 사용자 입력 지연을 측정하기 전에 전체 위젯 재작성으로 범위를 늘리지 않는다.

### 추가 프로파일 후보와 측정 계약

AA/가변 필압 긴 획의 반복 래스터(5절 측정 있음), 선택 clip path 재구성, 커버리지/합성 계획 반복, fill의 전체 캔버스 순회, 획별 임시 할당을 후보로 유지한다. O(n²) 또는 주 병목이라는 판단은 primitive 처리량·시간·할당 프로파일로 확인한다.

저장소 내 native benchmark를 먼저 마련한다. 외부 fixture 경로+SHA와 빌드/기기/OS/표시 조건을 받고 JSON 결과를 남긴다. 개인 문서는 동의 없이 커밋하지 않는다. 조건별 최소 50회·실행 순서 교차, cold/warm·open/undo 직후·immediate/idle·회전 0/5도·software/GPU·wobble ON/OFF·빈/실제 문서를 구분한다. p50/p95/max, UI/process CPU, worker·취소 지연·upload bytes·peak RSS를 기록한다.

웹의 과거 첫 획 1024² 2.7초 / 2048² 5.5초와 이후 시작 p95 0.3ms·commit p95 2ms는 당시 2,000획 조건의 기록이다. 현재 성능 보장이 아니다. 자동복구 serialize와 GIF의 Worker 점유도 별도 측정한다. 실제 펜 event→presentation, Metal trace, Windows 입력은 미검증이다. 전체 GPU 이관이나 파일 포맷 변경은 현재 우선 작업이 아니다.

## 6. 배포·플랫폼·유지보수

| ID·우선순위·상태 | 남은 일 | 완료 기준 |
|---|---|---|
| D06 · P1 · 부분 해결 | Windows 설치본에 [qt.conf](../resources/windows/qt.conf)를 두어 plugin 기준을 실행 파일 디렉터리로 제한한다. Qt는 원래 실행 파일 옆 경로뿐 아니라 설치 prefix도 탐색하므로 환경변수 정리만으로는 격리가 완전하지 않다. [Qt plugin deployment](https://doc.qt.io/qt-6/deployment-plugins.html), [Using `qt.conf`](https://doc.qt.io/qt-6/qt-conf.html). [TestWindowsPackage.ps1](../tests/TestWindowsPackage.ps1)은 PATH를 Windows 시스템 디렉터리만으로 다시 만들고 Qt/QML plugin 환경변수를 제거한 뒤 실제 `Ugurugu.exe`와 [PackageSmoke.cpp](../tests/PackageSmoke.cpp)를 실행한다. 설정·복구 경로도 임시 profile로 격리하며 CI 설치 트리와 최종 Velopack 설치본이 같은 검사를 사용한다. | 실제 앱 기동과 JPEG read-back이 성공해야 한다. `qwindows.dll` 또는 `Qt6Core.dll`을 뺀 임시 복사본은 반드시 실패한다. 실제 CI/release 성공을 확인하고, Velopack의 외부 `vcredist145-x64` prerequisite는 Qt·개발 도구가 없는 clean Windows 설치에서 별도 검증한다. |
| D09 · P2 · 미해결 | `.ugu` 파일 연결과 이미 실행 중인 앱에 두 번째 경로 전달 부재. [main.cpp](../src/main.cpp), [Info.plist.in](../resources/macos/Info.plist.in) | 새/기존 인스턴스에서 파일 열기, 공백·한글 경로·dirty 보호. `.wwpreset`은 지원 시 프로젝트와 다른 라우팅 |
| D10 · P2 · 검증 대기 (`5b34baa`) | 원인 확정(분석 보고서 R-01): CI·릴리스가 Qt Image Formats를 설치하지 않았고 macOS는 JPEG 플러그인만 복사했다. 모듈 설치·macOS GIF/WebP/TIFF 플러그인 동봉·런타임 지원 형식으로 만든 삽입 필터·패키지 smoke의 형식별 왕복으로 수정. macOS 설치 번들 smoke 통과, WebP 플러그인을 뺀 음성 대조는 실패 | Windows CI 패키지와 배포 Qt 6.11.1 macOS 번들에서 같은 smoke 통과 |
| D15 · P2 · 부분 해결 | Qt 기본 번역은 `5b34baa`에서 해결(분석 보고서 U-01: windeployqt는 `qt_<lang>.qm`으로 합쳐 배포, macdeployqt는 미배포, 앱은 `qtbase_<lang>`을 Qt 설치 경로에서 찾았다. ko/ja `qtbase` 카탈로그를 앱 리소스에 넣고 `InterfaceTranslators`로 설치). Wawa·프리셋·복구 오류 literal(U-14), numerus 점검은 남음 | ko/en/ja 오류·파일 dialog·복수형 실제 표시. 번역 추출 100%와 사용자 경로 완전 번역을 구분 |
| D18 · P2 · 미해결 | Windows 업데이트 확인 busy 중 수동 요청이 무시될 수 있고 자동 offer가 시작 dialog와 겹침. 취소/다운로드 상태도 확인 | 자동→수동 요청, 시작 dialog 중 offer, 네트워크 실패·취소·재시도·설치 경로. 보관하지 않은 installer로 ‘나중 설치’를 약속하지 않음 |
| D22 · P3 · 구조 제안 | Canvas/App의 공유 상태, 액션 중복 계산·이름 재탐색, global filter, controller/serializer 결합 | session identity·pending edit·queue·채택 규칙 테스트 후 한 경계씩 분리. 매크로 staged document 노출은 현재 소비자 계약을 먼저 확인 |
| D23 · P3 · 정리 후보 | ToolPopover 호출 여부, 마스크 분기 조기 반환 뒤 코드, 이벤트 중복, mask/transform/선택 helper 중복 | 모든 지원 빌드/호출자를 확인하고 제거. 샘플러·렌더 품질·epsilon의 의미가 다른 코드는 모양만 보고 합치지 않음 |
| D24 · P3 · 검증 개선 | 전체 line coverage 70% 게이트 외 위험 경로 회귀, 짧은 fuzz의 corpus 보존, TSan 활용, 고정 대기·dialog fallback 개선, 플랫폼별 검사 공백 | 취소·세대 테스트가 전혀 없다고 단정하지 말고 기존 테스트별 보장 확인. 조건 기반 대기·결과 artifact·장시간 fuzz를 위험도에 따라 추가 |
| D25 · P3 · 부분 해결 | 이번 문서 통합·이력 정정은 완료. root README 세 언어 기능/단축키, BUILDING preset/target, schema 명세, release note 색인·표제 정돈은 후속 | 실제 action/CMake와 대조하고 세 언어 일치. 역사적 commit 메시지 불일치 기록은 남기되 이력 재작성은 하지 않음 |

배포 유지 사항: 의존성 URL/hash·Actions SHA 고정, main CI 통과 확인, macOS 서명·공증·Gatekeeper·rpath 감사, 테스트별 설정/복구 경로 격리, 번역·SPDX 게이트. 검증을 편하게 하려고 완화하지 않는다.

배포 후속은 아직 남아 있다. 2.2.10의 당시 CI·release 성공 기록은 실제 **2.2.9→2.2.10 updater UI·설치 완료·재실행** 검증을 대신하지 않는다. Windows 실기기 설치/업데이트도 별도 확인한다. 2.2.11부터 릴리스 노트는 영어 한 파일이고 업데이트 창은 노트를 보여주지 않는다. Windows는 업데이트·취소 두 버튼이고, macOS는 Sparkle 기본 창에서 노트만 숨긴다(`SUShowReleaseNotes`). 업데이트 피드에도 노트를 싣지 않는다.

2026-09-18 `npm audit`은 현재 lockfile의 전이 의존성 `devalue 5.9.0`과 `nanoid 3.3.17`에 각각 [입력 기반 DoS](https://github.com/advisories/GHSA-9rgm-9g3h-6x36), [0 크기 custom generator 무한 반복](https://github.com/advisories/GHSA-2v37-7h3g-55p8) 권고를 보고했고 자동 수정 가능 버전은 제시하지 않았다. 둘을 실제 제품 취약점으로 단정하거나 검증 없이 override하지 않는다. 업스트림 호환 버전과 브라우저 런타임 도달 가능성을 확인하는 별도 의존성 후속으로 남긴다.

Qt 번들 내 외부 구성요소 고지·소스/재링크 제공 범위, 웹 정적 링크 의무, Android APK 교체/재패키징 경로는 실제 artifact 기준으로 감사한다. [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)의 존재만으로 충족 또는 위반을 판정하지 않는다. SBOM·CMake 의존성 갱신 점검은 후속 개선안이다.

## 7. 웹 제품의 완료 범위와 남은 작업

### 구현·과거 검증 기록이 있는 범위

공용 WASM 엔진·ABI 9, Worker 큐, WebGL/Canvas2D fallback, 그리기·지우기·채우기·선택/변형·레이어/그룹·undo/redo, 재생·프레임/fps·캔버스 크기, 같은 탭 clipboard, 모바일 dock/sheet·두 손가락 제스처, 파일 다운로드·복구·고지 UI가 있다. 9월 8일에는 텍스트 윤곽의 일반 획 변환, 레이어별 모션, PNG/JPEG/WebP 이미지 삽입이 추가됐다. 이 목록은 모든 경계가 무결하다는 판정이 아니다.

웹 텍스트는 Pretendard JP / OpenType.js 경로이며, 이미지 삽입과 `.ugu` 파일 열기의 메모리 정책은 서로 다르다. 삽입 완료를 R11 해결로 세지 않는다. iPhone 부팅·그리기·모바일 감지와 itch.io 스테이징/전체화면은 과거 확인 기록이 있다.

### 코드로 처리할 잔여

- 3–5장의 웹 저장·복구·입력·메모리 항목을 먼저 닫는다. “남은 일 대부분이 실기기·계정뿐”이라는 과거 설명은 현재 미해결 목록과 맞지 않는다.
- 선택 삭제 후 `installSelection(..., QImage(), ...)`로 지운 선택을 undo가 어떤 단위로 복원할지 정한다. 픽셀과 선택 상태를 한 사용자 작업으로 검사한다. [EngineBridgeSelection.cpp](../src/wasm/EngineBridgeSelection.cpp).
- GIF 진행률·취소와 작업 분할, snapshot의 입력 경쟁을 다룬다. render+encode가 Worker를 막는 동안 셸만 반응하는 것으로 작업 취소 완료를 주장하지 않는다.
- 모바일 touch target·시트 스크롤·레이어 키보드 조작·스크린리더 순서·지속적인 복구 실패 안내/live region을 다듬는다.
- 공개 배포용 strict 검사: 엔진 파일 존재, ABI, 실제 boot·open·draw·save를 필수 확인한다. `check_itchio_package.mjs`의 파일 형식 검사와 WASM 없는 셸 개발 모드는 별도로 유지한다.
- 개발 중 WASM 재빌드 후 `npm run sync-engine`이 필요하다. ABI가 같아도 낡은 엔진 artifact가 남을 수 있으므로 빌드 식별·검증 방식을 정한다.
- EngineClient/Worker/C ABI의 명령·응답·오류 계약과 진단 정보(앱/ABI/렌더러/프로파일/마지막 성공 revision)를 관리한다. 기본 진단에 문서 본문·개인 경로를 수집하지 않는다.

### 2026-10-05 웹 정적 분석 — 새 발견 W-01–W-16

기준 `143931c`. `web/`·`src/wasm/`·워커·웹 CI를 코드로 추적한 결과다. 이 환경에는 WASM 툴체인이 없어 **실행 재현은 하지 않았다**. "확인"은 코드 경로를 끝까지 따라간 것, "추정"은 실행으로 증명해야 하는 것이다. 기존 ID와 겹치는 부분은 해당 ID에 적고 여기에는 새 원인만 둔다.

기존 ID의 현재 상태: R10·R11(`onFileChosen`이 `file.arrayBuffer()` 후 크기 검사) 모두 그대로 남아 있다. R08은 button Enter/Space는 고쳐졌고 range·checkbox의 방향키·Enter·Delete가 앱 명령으로 가는 문제가 남았다(`Shortcuts.ts`의 `isTextEntry`).

| ID·우선순위·상태 | 근거와 원인 | 조치·완료 기준 |
|---|---|---|
| W-01 · P1 · 해결(`c610588`, 장애 주입 검증 2026-10-05; 실제 heap 고갈은 미측정, main `d182672`) | (수정 전) [engine-worker.js](../web/public/engine/engine-worker.js)의 `openDocument`·`withPoints`·`layerRename`이 `_malloc` 결과를 검사하지 않았다. `ALLOW_MEMORY_GROWTH=1`([UguruguWasm.cmake](../cmake/UguruguWasm.cmake))에서 실패한 malloc은 0을 반환하고, `HEAPU8.set(bytes, 0)`이 정적 데이터·스택을 덮는다. 새 문서를 연 뒤 이전 문서를 닫으므로 큰 문서가 열린 상태의 큰 파일 열기에서 도달 가능하다. `text`·`insertImage`는 이미 검사한다 | 모든 할당 null 검사와 `try/finally` 해제. 512MB 근처에서 64MiB 열기가 명시적 오류로 끝나고 기존 문서가 유지됨 |
| W-02 · P1 · 해결(`c610588`, 장애 주입 검증 2026-10-05, main `d182672`) | (수정 전) 예외 모드가 없어 C++ throw(`qBadAlloc` 등)는 `abort()`가 된다. 워커에 `onAbort`가 없고 catch-all이 `RuntimeError`를 일반 `ok:false`로 돌려준다. [EngineClient.ts](../web/src/lib/EngineClient.ts)의 `#fail`은 `worker.onerror`에서만 불리므로 셸은 죽은 엔진에 계속 명령을 보내고, 다음 자동저장이 반쯤 바뀐 문서를 단일 복구 슬롯에 쓸 수 있다 | abort를 치명 상태로 전파해 이후 요청·자동저장 차단, 사용자에게 마지막 정상 복구본 안내. 강제 abort 시나리오 브라우저 회귀 |
| W-03 · P1 · 미해결(한도 확인, OOM 추정) | 웹 열기는 파일 바이트(64MiB)만 보고, 엔진은 데스크톱 `DocumentLimits`(4096², 마스크 표 256MB, 래스터 256MB decoded)를 쓴다. 0 마스크는 약 1000:1로 압축되므로 작은 파일이 512MB heap을 넘길 수 있다(계산상 추정). [EngineBridge.cpp](../src/wasm/EngineBridge.cpp)의 열기는 입력을 한 번 더 복사한다. zlib 출력 자체는 `BoundedCompression`이 막는다 | 웹 전용 decode 예산(캔버스·마스크·래스터 총량)을 채택 전에 적용. R11과 함께 닫음. WASM 열기 경로 fuzz |
| W-04 · P1 · 해결(`58e6aac`, 브라우저 회귀 2026-10-05, main `04aa1d6`) | (수정 전) `autosave.start()`가 복원/버리기 배너가 결정되기 전에 시작된다. 배너를 둔 채 한 획만 그어도 15초 안에 단일 `slot`이 새 문서로 바뀌고, 다시 새로고침하면 이전 작업은 사라진다. 두 탭은 서로의 슬롯을 덮고 다른 탭의 살아 있는 문서를 "이전 세션"으로 제안한다 | offer 결정 전 슬롯 보호, 탭·세션별 키. R04의 세대 설계와 함께 |
| W-05 · P1 · 해결(`58e6aac`, 브라우저 회귀 2026-10-05, main `04aa1d6`) | (수정 전) [AutosaveController.svelte.ts](../web/src/lib/AutosaveController.svelte.ts)의 `restore()`는 offer를 비우고 bytes를 워커로 transfer한다. 열기가 실패하면 배너와 메모리 사본이 모두 없다. `discard()`는 현재 세션이 방금 쓴 snapshot도 지우고, `savedRevision`이 같아 다음 편집 전까지 다시 쓰지 않는다 | 열기 성공 뒤에만 offer 소비, 복사본 전달. discard는 offer 출처만 삭제 |
| W-06 · P2 · 미해결(추정) | `onWindowBlur`가 `drawing`·`panning`·`picking`·`activeTouches`를 초기화하지 않고 `lostpointercapture` 처리가 없다. pointer-up을 잃으면 `ready()`가 계속 false라 자동저장이 멈추고, 대기 중 stroke 작업은 문서 세대 검사 없이 새 문서에 적용된다. 잃은 touch-up 뒤 한 손가락이 pinch로 처리된다. `strokeAppend`/일괄 `strokeEnd`는 `frameIndex`를 실행 시점에 읽어 그리는 중 재생과 어긋날 수 있다 | D03과 같은 정책으로 웹 입력 종료 정의. stroke 작업에 시작 시 frame·세대 캡처 |
| W-07 · P2 · 미해결(확인) | 크기 dialog·모바일 Sheet가 열려도 App 단축키(Delete, Ctrl+Z, Enter, Ctrl+O)가 배경 문서에 적용된다. Sheet의 Escape는 window 리스너에서 `stopPropagation`만 해 App 핸들러도 실행된다. WobblePanel의 `<summary>`는 Enter/Space가 재생·팬으로 가서 키보드로 열 수 없다 | R10과 함께. 모달 열림 상태 하나로 단축키 차단 |
| W-08 · P2 · 미해결(확인) | 셸↔워커 프로토콜 버전 검사가 없다. 워커·엔진은 해시 없는 고정 경로라 캐시된 옛 워커가 새 셸과 짝지어질 수 있고, 바뀐 필드는 `undefined`→0으로 엉뚱한 레이어·프레임을 바꾼다 | 워커가 프로토콜 버전을 보고하고 셸이 거부. 엔진 파일명에 빌드 식별 포함 |
| W-09 · P2 · 미해결(확인) | [EngineBridgeExport.cpp](../src/wasm/EngineBridgeExport.cpp)의 `exportBytes`·`serialized`가 JS 복사 뒤에도 handle에 남아 512MB 예산을 차지한다. GIF 내보내기는 전 프레임을 메모리에 렌더(128MiB 상한)하며 워커를 막는다. 워커 응답 타임아웃·생존 확인이 없다 | 복사 뒤 즉시 해제, 데스크톱처럼 프레임 순차 공급. 7장의 GIF 진행·취소 항목과 함께 |
| W-10 · P2 · 미해결(확인) | 웹 PNG 내보내기는 premultiplied BGRA를 8비트로 되돌린 뒤 `putImageData`→`toBlob`([CanvasPresenter.ts](../web/src/lib/CanvasPresenter.ts))을 거친다. 브라우저가 다시 premultiply하므로 반투명 픽셀이 데스크톱 PNG와 다를 수 있다. 미리보기=최종 계약의 웹 PNG 경로 위반 가능성 | 엔진 PNG 인코딩으로 통일하거나, 저알파 fixture로 네이티브와 바이트·픽셀 비교 |
| W-11 · P2 · 미해결(확인) | `release.yml`에 웹 빌드·게시 단계가 없어 itch.io 패키지가 커밋과 연결되지 않는다. `check_itchio_package.mjs`는 엔진 없는 `web` 잡에서만 돈다. [sync-engine.mjs](../web/sync-engine.mjs)는 엔진·zlib 라이선스가 없어도 경고만 한다 | 7장의 strict 검사를 실제 엔진 포함 산출물에 적용하고 CI artifact로 보관 |
| W-12 · P2 · 미해결(확인) | CI가 `playwright@1.62.1 install chromium`을 쓰는데 lockfile은 `playwright-core` 1.63.0이다. 리비전이 다르면 하니스가 러너의 시스템 Chrome으로 조용히 바뀐다. emsdk는 HEAD를 clone한다(SDK 4.0.7만 고정) | 두 버전 일치·불일치 시 실패. emsdk 커밋 고정 |
| W-13 · P2 · 미해결(일부 추정) | 웹 고지: [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)의 Svelte 5.56.8(실제 5.57.1), `clsx` 누락, 정적 링크된 Qt 내장 서드파티·Emscripten libc++/musl은 링크로만 안내. [NoticesDialog.svelte](../web/src/lib/NoticesDialog.svelte)의 소스 링크가 태그·커밋이 아닌 저장소 루트. Pretendard 서브셋의 OFL 예약 이름 사용 여부 확인 필요 | 실제 웹 artifact 기준 감사(6장 고지 항목과 함께) |
| W-14 · P3 · 미해결(확인) | JS 입력 검증 공백: `ugu_set_brush` width NaN·색 범위, stabilization 강도, 음수·NaN timestamp의 `quint64` 변환(UB), 짧은 points 배열. 셸 코드에서만 오는 값이라 파일로는 도달 불가 | 브리지 경계에서 유한값·범위 검사 |
| W-15 · P3 · 미해결(확인) | CSP·보안 헤더 없음(XSS 싱크는 없음, itch.io는 헤더 불가라 `<meta>` CSP). 레이어 이름 변경이 `window.prompt`뿐이라 `allow-modals` 없는 iframe에서 조용히 실패. DPR만 바뀌는 모니터 이동은 ResizeObserver로 감지되지 않음. ColorWheel ARIA·status live region 없음. [SECURITY.md](../SECURITY.md)에 웹 빌드 범위 없음, 웹 `package.json` 버전 0.1.0 | 항목별 |
| W-16 · P3 · 미해결(확인) | 테스트 공백: Chromium만, 메모리 한계 근처·대용량 열기 없음, WASM 열기 fuzz 없음, 교차 출처 iframe·하위 경로 호스팅 없음 | 위 W 항목의 회귀와 함께 추가 |

확인한 정상 경로: XSS 싱크(`{@html}`·`innerHTML`·`eval`) 없음, 이미지 삽입의 크기·헤더 치수 선검사와 ImageBitmap 해제, 번들 폰트만 파싱, object URL 해제, 레이어 명령의 id 해석, WebGL context loss 대체, 재생 backpressure, 워크플로 `contents: read`·액션 SHA 고정·`pull_request_target` 없음.

데스크톱 대비 웹에 없는 것(아래 선택 기능 외): `.wawa` 열기, JPEG·WebP 내보내기, GIF 배율·투명도 옵션, 시스템 폰트, 변형 샘플링 선택, `.wwpreset`, 단축키 재지정, 필압 끄기·펜 지우개 끝, bmp/gif/tiff 삽입. 되돌리기는 개수(64/32)만 있고 바이트 예산이 연결되지 않는다(R11). root README 세 언어는 웹 빌드를 언급하지 않는다(D25).

### 실기기·배포 완료 기준

Android Chrome·iPad, 데스크톱 Chromium/Firefox/Safari/Edge 지원 행렬, visibility 복귀·다운로드·복구·낮은 메모리를 확인한다. iframe 내부 단축키·새로고침 후 IDB·PNG/GIF 권한·실제 배포 고지도 별도 시험한다. 자동 WebGL context loss→fallback 검사는 실제 driver loss 경험과 구분한다. Mobile Friendly 표시는 확인한 범위에 맞춘다.

### 선택 기능과 범위 밖 항목

배경색, stroke 속성 편집, 이미지 전용 변형 UI, canvas mirror, 브러시 UI 동등성, Wawa import는 후속 기능이다. File System Access의 같은 파일 재저장은 선택 기능이며 OS clipboard는 원래 MVP 제외다. 웹은 **영어 통일 결정**을 유지하고 번역 누락 버그로 분류하지 않는다. GPU 최적화는 프로파일 후 결정한다.

초기 타당성 보고서의 Qt Widgets 전체 이식/별도 엔진 재작성 비교는 역사적 결정 근거다. 현재 선택은 공용 엔진+웹 셸이므로 당시 인월·주차 추정과 미구현 단계 목록을 현재 일정으로 재사용하지 않는다. itch.io 제한·브라우저 정책·외부 라이선스 조건은 배포 시점에 공식 문서로 다시 확인한다.

## 8. Android V1 계획 — 구현 전

이 절은 2026-09-16 문서에 기록된 사용자 요구와 제안을 보존한다. 별도 저장소 생성·도구 설치·APK 제작을 이번 문서 작업이 승인하거나 실행한 것은 아니다.

### 요구사항과 경계

갤럭시 탭 S8 이상 태블릿, S Pen 필기감 최우선, PC 주요 기능, 가로/세로/분할 화면, 필압·버튼 지우개·호버·손바닥 차단을 목표로 한다. 손가락 그리기·iPad·구형 reader와 `.wawa/.wagle/.wobble` import는 제외한다. 현행 `.ugu` 양방향 왕복, 내부 작업 목록·자동 보관·외부 import/export, ko/en/ja를 유지한다. V1은 로그인 없는 로컬 앱이며 cloud는 후속이다.

계획 저장소는 별도 비공개 `nyabi-gh/Ugurugu-Android`, 개인·지인용 무료 APK부터 시작한다. 코드 권리·아이콘 허락은 당시 사용자 확인 기록이며 외부 Qt·폰트·codec 의무와는 별개다. 원본 commit·patch 목록·제외 파일을 `UPSTREAM` 기록으로 관리해 공용 수정 누락을 막는다. 공개 앱의 기존 라이선스/헤더를 근거 없이 일괄 변경하지 않는다.

S8 8GB를 기준 기기로 **제안**했고 보유 기기는 S9 추정이다. 연결 시 모델·RAM·OS·density·주사율을 확인한다. S9 통과를 S8 지원 완료로 바꾸지 않으며 FE/Lite/A를 자동 포함하지 않는다. 납기·진척률을 임의로 약속하지 않는다.

### 기술·입력·UI 계약

- 우선 후보: Qt Quick/QML + C++ canvas와 공용 엔진. 필요한 MotionEvent 정보를 보강한 뒤에도 병목이 UI 입력/표시에 남을 때 Kotlin UI 대안 PoC를 비교한다. 엔진 전체 재작성은 기본안이 아니다.
- pointer ID·timestamp·historical sample·pressure·hover·button·cancel을 보존한다. Qt/JNI 중복 입력을 금지한다. 버튼 중간 전환은 기존 획을 경계에서 종료하고 새 모드로 시작한다.
- 한 손가락은 그리지 않고 두 손가락은 pan/zoom/rotate. 펜 우선 palm 처리, pointer별 cancel, 시스템 제스처 취소를 시험한다. OS cancel은 임시 입력 폐기, 일반 포커스 상실은 D03과 함께 정책을 확정한다. 모든 비활성화를 일괄 커밋/폐기하지 않는다.
- 예측점은 도입하더라도 preview 전용이며 저장·history에는 실제 점만 포함한다. Android Ink 기본 붓으로 표현을 바꾸는 일은 별도 결정이다.
- 현재 창 크기 기준 rail/패널/시트 전환, 좌우 배치 설정, IME·inset·뒤로 가기, 큰 글씨·한글 조합·일본어 변환을 지원한다. 초기 폭 구간 600/840·터치 영역 48dp는 실기기 조정용 제안이다. 레이아웃 변경이 문서·pending edit를 임의 확정하지 않는다.

### 저장·호환·내보내기

ProjectStore는 project ID별 snapshot·metadata·thumbnail, 세대별 완전 기록 후 현재 포인터의 원자적 교체, 직전 정상 세대 보존을 기본안으로 한다. 목록/썸네일 손상 시 본문 복구, 삭제의 복구 가능 보관도 필요하다. 내부 ID·시각 등은 `.ugu` 밖에 둔다.

자동저장 제안은 idle 약 1.5초·연속 작업 중 약 10초지만 실측으로 조정하며 무손실 시간 보장이 아니다. project/session/revision/generation을 함께 사용하고 pen-down에 전체 serialize를 실행하지 않는다. 작업 전환은 저장 실패를 처리하고, process kill은 마지막 완전 snapshot으로 복구한다. destructor/onDestroy만 믿지 않는다.

SAF의 `content://`는 일반 파일 경로가 아니다. 누적 읽기 상한·provider 취소/권한·미상 크기를 처리한다. 완전한 내부 출력 후 외부 URI에 복사하며 외부 provider까지 원자적이라고 가정하지 않는다. 공유 창 표시와 수신 앱 업로드 완료도 구분한다.

PC→Android 편집/출력→PC→Android 왕복에서 hierarchy·UUID·seed·필압·alpha·선택/변환·모션·프레임/fps를 보존한다. 현재 포맷과 미래 schema 거절·구형 제외 테스트를 분리한다. 한도 초과는 원본 보존 후 안내하고 자동 축소 덮어쓰기는 하지 않는다.

PNG/JPG 정확 렌더, GIF 순차 공급·취소, WebP encoder 내부 peak, alpha·duration·loop를 검사한다. preview 축소 결과를 고품질 출력으로 재사용하지 않는다. V1은 보이는 동안 export를 완료하는 정책이며 완전 background에서는 취소·재시작, 부분 파일 비채택을 기본안으로 한다. 장시간 background 서비스는 후속이다.

### 초기 성능·빌드 제안 — 달성 결과 아님

| 영역 | 시작안과 검증 |
|---|---|
| 문서 | 기본 1600×1200·30프레임·12fps, 신규 최대 2048²부터. PC 4096 문서는 별도 admission. 레이어 8/16/32 측정 |
| 예산 | undo 64단계+64MiB, preview 전체 128MiB, decode 64MiB, serialize cache 16MiB, export 192MiB부터. 동시 사용·GPU·encoder 포함 재조정 |
| 메모리·동시성 | 일반 PSS 512MiB / stress 768MiB 최초 목표, warmup 1–2개부터. 보장치나 모두 선할당할 용량이 아님 |
| 지연 | event→presentation p95 33ms, UI batch 4ms, 60Hz frame·보통 commit 16.7ms 최초 목표. 120Hz 8.3ms는 별도. cold/open/undo·발열을 분리 |
| 내구·정확성 | 30분 이상·오입력/중복/cancel 오커밋·단조 메모리 증가·ANR 검사. 실제 펜과 자동 궤적 모두 사용 |
| 도구 | arm64-v8a, Qt 6.11.1 비교 시작, NDK r27c·JDK 21·API 36 target / API 31 최소는 당시 후보. 실제 채택 키트와 공식 지원 조합 재확인 후 고정 |
| 패키지 | 모든 Qt/plugin/native `.so`와 APK의 16KB 정렬·실행, clean install·동일 서명 업데이트·작업 보존. 키는 저장소 밖 보관 |

기기·OS·전원·온도·주사율·빌드 hash와 p50/p95/max를 남긴다. CPU 함수 시간만으로 펜 지연을 보고하지 않고 event/Qt/patch/upload/presentation 시계를 맞추며 실제 화면 지연도 확인한다. 표준 fixture는 가벼운 1024², 2048² 혼합 레이어, 2,000획·200,000점 stress, 4096² 이미지·mask·group과 동의된 사용자 문서다.

### 구현 순서와 완료 게이트

| 단계 | 산출물 | 완료 조건 |
|---|---|---|
| A0 범위·권리·이관 | 저장소 정책·UPSTREAM·현행 fixture·제외 목록 | 파일 계약과 외부 고지/배포 경로 확정 |
| A1 기술 검증 | arm64 debug APK·엔진 프레임·펜 진단·기본 파일 왕복 | 실제 기기에서 pressure/hover/button/cancel 로그·표시 |
| A2 필기감 경로 | 증분 patch→texture·trace·palm/gesture | 지연/정확성 측정 후 Qt 경로 유지 또는 제한된 대안 비교 |
| A3 공용 경계 | 세션 분리·bounded decode·변환 수정·구형 reader 제거 | 코어 회귀·현행 파일 호환. 공용 보안/정확성 수정은 이 단계까지 미루지 않음 |
| A4 편집 UI | 도구·색·레이어·모션·timeline·세 방향/분할 | 화면만으로 주요 편집 과업 완료 |
| A5 저장·복구 | ProjectStore·snapshot·SAF·lifecycle | kill·공간 부족·write 실패·전환에서도 정상 작업 보존 |
| A6 기능·공유 | 선택·변형·이미지·텍스트·출력·취소 | PC 왕복과 편집→저장→공유 완주 |
| A7 품질 | S8·언어·접근성·메모리·발열·장시간 | 중대한 입력/복구/호환 문제 해소, 미검증 환경 명시 |
| A8 배포 | 서명 release APK·고지·설치 안내 | 새 설치·동일 키 업데이트·작업 유지·사용자 실측 |

호스트 unit/golden·QML 검사→APK build→에뮬레이터 lifecycle/파일/페이지 크기→서명 검사를 자동화한다. 에뮬레이터는 S Pen 수용 검증을 대신하지 않는다. APK 전달→사용자 고정 시나리오→로그/사용감→수정의 반복으로 진행한다. cloud·계정·비용·동기화 충돌은 V1 이후 별도 승인 범위다.

## 9. 다음 실행 순서와 닫는 기준

### 분석 보고서 발견의 진행 (2026-10-05, macOS)

[ANALYSIS_REPORT.md](ANALYSIS_REPORT.md) §11 "바로 고칠 것"을 데스크톱 우선으로 처리했다. 웹 항목은 후순위로 미뤘다. 각 수정은 수정 전 실패하는 회귀를 같은 설정의 별도 worktree에서 확인했고, offscreen CTest 13/13과 cocoa 플랫폼 UI 스위트를 통과했다(증거 범위 표의 2026-10-05 행).

| ID | 상태 · commit | 원인과 수정 | 남은 검증 |
|---|---|---|---|
| B-01, B-02 | 해결 (`c247645`, `00eac5d`) | 4절 참조 | — |
| B-03 | 해결 (`710e33f`) | 그림자 pixmap이 윤곽 경계 사각형 전체였다(4096² 1600%에서 17GB 할당, macOS peak RSS +5.7–6.2GiB). 그림자 alpha는 이동한 윤곽까지 거리만의 함수이므로 변 gradient 띠와 모서리 radial로 직접 칠한다. 캐시 없음, 기존 14패스와 alpha 차이 최대 3/255. 캔버스 clip 영역도 위젯과 교차한 윤곽으로 만든다. Release 팬(30°) 50% 1.74→1.77ms, 400% 8.0→0.86ms, 1600% 110→0.87ms | Windows 실측 |
| U-01 | 해결 (`5b34baa`) | D15 참조 | Windows·macOS 배포본 ko/ja 표준 버튼 육안 확인 |
| R-01 | 검증 대기 (`5b34baa`) | D10 참조. LibTIFF 고지 추가 | Windows CI 패키지 |
| U-02 | 해결 (`22c654a`) | `QKeySequence(StandardKey)`가 첫 바인딩만 썼다. 표준 키 액션은 `keyBindings()` 전체를 기본+별칭으로 등록(Windows Redo의 Ctrl+Shift+Z, Cut/Copy/Paste 보조 키) | Windows 실키 확인 |
| B-15 | 해결 (`ed37683`) | 텍스처 교체마다 바인딩 객체를 새로 만들어 파이프라인이 해제된 객체를 가리켰다. 객체를 유지하고 `setBindings`+`updateResources`로 리소스만 교체 | Windows D3D11 창 크기 변경 |
| P-05 | 해결 (`5d2693a`) | `Document::isLayerDescendantOf`가 호출마다 계층 전체를 분석했다. split은 부모 맵으로 루트를 한 번씩 찾고, `supportsLayerSplit`은 필요할 때 한 번만 분석. 226레이어 64×48 split 156→2.6ms, 결과 동일 | WASM build/parity(툴체인 없음) |
| B-08 | 검증 대기 (`8edf708`) | 취소할 수 없는 Velopack 확인·다운로드가 종료 시 join하는 전역 풀에 있었고, 그동안 인스턴스 락을 쥐었다. 전용 풀(종료 시 기다리지 않음)로 옮기고 창·업데이트 컨트롤러 정리 직후 락을 푼다 | Windows 빌드·오프라인 종료 후 재실행. 다운로드 취소 버튼(D18)은 Velopack API에 취소가 없어 남음 |
| B-06, B-07 | 해결 (`0f826e3`) | 접촉 중 측면 버튼 Press가 획을 취소했다 → 무시하고 팁 Release만 시퀀스를 닫는다. 근접 이탈이 포커스 상실과 같은 초기화로 Shift·Space까지 풀었다 → 포인터 취소와 키 수정자 해제를 분리하고 근접 이탈은 호버 링도 숨긴다 | 실제 펜(WinTab·macOS)에서 측면 버튼·근접 순서 확인(인수인계 1번) |
| L-01, L-03, R-13 | 부분 해결 (`af980d2`) | Qt 버전·모듈·소스 위치·Qt 내장 서드파티 목록 고지. `opengl32sw.dll` 동봉 중단(`--no-opengl-sw`, Windows 패키지 테스트가 부재 확인). smoke 필수 라이선스에 LGPL·libwebp 추가 | `D3Dcompiler_47.dll`·`Qt6Network.dll` 동봉 필요성 판단, About Qt 버튼, L-02 폰트 고지 |
| Q-03 일부 | 보류 | `StrokePresence` 이펙트는 구독자가 없지만, 같은 마스크 패킹이 선택 픽셀이 없는 클립 마스크 획을 `RejectedMaskLimit`로 거절하는 역할을 겸한다. 그 결과 코드를 정한 뒤 제거한다 | — |

테스트 하네스 정리 (`295fdd6`): cocoa에서만 실패하던 UI 테스트 4건은 모두 테스트 쪽 문제였다(기본 대화상자 `reject()`, 72dpi에서 11px 최소치 아래인 9pt 기준 폰트, macOS가 이동 이벤트로 바꾸지 않는 `QCursor::setPos`, GPU 표시에서 성립하지 않는 software 체커 전제).

1. **실기기 확인과 안전한 입력·배포:** 위 표의 Windows·실제 펜 검증, D04/R01 WASM parity, D16 Sparkle 실제 업데이트, D05 기본 대화상자 수용, D06 clean Windows 설치.
2. **문서 정확성·보존:** 분석 보고서 B-04(그리는 중 합성과 최종 렌더 차이), B-09–B-14(열기·히스토리·저장·복구 실패 경로), R06, D07·D19·D20. 웹 R03–R05는 아래 웹 수정 순서를 따른다. 문서 identity와 pending edit 정책을 공유하되 한 번에 전부 리팩터링하지 않는다.
3. **입력·액션·접근성:** D03·D11–D13·D17, R07–R10·R12–R14, D15·D18. 실제 입력과 저장 결과를 함께 확인한다.
4. **측정 기반 성능:** 기존 두 구현의 A/B, D08·D14·D21·R11과 웹 Worker 점유. 측정 뒤에만 최적화 경계를 선택한다.
5. **제품 확장·유지보수:** 웹 선택 기능, D09·D10 artifact 점검, D22–D25의 남은 작업, Android A0–A2부터. 독립적으로 가능한 검증은 앞 단계와 병행할 수 있다.

2026-10-05부터 웹 코드 수정을 시작한다. 순서: W-01·W-02(워커 메모리·abort) → R09 → R04·R05·W-04·W-05(복구·문서 교체) — 여기까지 2026-10-05 main 병합 — → **다음: W-03·R11**(웹 decode 예산) → W-06–W-10 → W-11–W-13(배포·CI·고지). 공용 엔진을 건드리면 위 공통 완료 규칙대로 native suite와 WASM parity를 함께 확인한다.

#### W-01·W-02 진행 (2026-10-05, 브랜치 `web/engine-worker-safety`) — 수정 완료, main 병합

- 환경: Windows 11에서 emsdk 4.0.7 + Qt 6.11.2 `wasm_singlethread`(호스트 Qt 6.11.2 msvc)로 엔진을 직접 빌드했다. `wasm-release` 프리셋은 macOS 전용이라 같은 옵션으로 수동 구성했다. 브라우저 시나리오를 실엔진으로 로컬에서 돌릴 수 있다.
- 회귀: 하니스 `engineFault()`가 엔진 로더 뒤에 장애 주입 코드를 붙인다(`_malloc` 0 반환, export에서 `WebAssembly.RuntimeError`). [22-heap-allocation-failure.mjs](../web/tests/scenarios/22-heap-allocation-failure.mjs), [23-engine-abort.mjs](../web/tests/scenarios/23-engine-abort.mjs).
- 수정 전 결과: heap 1건 실패. 상태가 `Open failed: illegal value (code 3)`로 나온다. 파일 바이트를 주소 0에 쓴 뒤 엔진이 다른 내용을 읽은 것이며, 메모리 부족으로 보고되지 않는다. abort 3건 실패. 치명 상태 안내가 없고, abort 뒤에도 획이 그려지며, 다음 자동저장이 복구 슬롯을 덮는다. 대조군 `failed-open`은 통과했다.
- 수정(`c610588`, 웹 셸만 변경, 공용 엔진 C++ 변경 없음):
  - 워커의 모든 힙 복사(open·selection points·text·insertImage·layerRename)를 `withHeapCopy` 하나로 모았다. `_malloc`이 0이면 `out of memory` 오류로 끝나고, 복사본은 `finally`에서 해제한다. 열기는 새 문서를 만든 뒤에만 이전 문서를 닫으므로 실패해도 기존 문서가 남는다.
  - 엔진 로드 직후 `_` 접두 export를 모두 감싼다. export 밖으로 예외(abort·trap)가 빠져나오면 `engineFailure`를 기록하고, 이후 export 호출과 요청은 모두 `fatal` 응답으로 거부한다. 엔진 로드 실패·ABI 불일치도 같은 상태가 된다. 죽은 엔진에는 `_free`도 부르지 않는다.
  - `EngineClient`는 `fatal` 응답에서 `#fail`로 대기·이후 요청을 모두 거부하고, 생성자 콜백으로 App에 한 번 알린다. `worker.onerror`(엔진 파일 누락)도 같은 콜백을 탄다.
  - App은 `Engine stopped: … Reload the page …` 메시지를 이후 모든 상태 메시지보다 우선해 보이고, 재생을 멈추고 다시 시작하지 않으며, 자동저장 `ready`를 false로 두어 마지막 정상 복구본을 지킨다.
- 수정 후 결과(같은 로컬 엔진 빌드, Chromium, 2026-10-05): `heap-allocation-failure` 3/3, `engine-abort` 4/4 통과. 전체 browser suite 23개 시나리오 통과, `npm run check` 0 오류·0 경고. 같은 빌드에서 수정 전 코드로 되돌려 다시 돌리면 위 기록대로 heap 1건·abort 3건이 실패한다.
- 남은 것: 실제 heap 고갈(512MB 근처에서 64MiB 열기)은 장애 주입으로 대신했고 실측하지 않았다. 이 상한은 W-03의 웹 decode 예산과 함께 다룬다. CI `wasm` 잡은 main 병합·푸시 뒤 확인한다.
- 2026-10-05 main에 fast-forward 병합·푸시(`d182672`). R09와 함께 푸시한 `2f33138`의 CI는 Wasm engine parity·Web shell을 포함한 13개 잡이 모두 성공했다.

#### R09 진행 (2026-10-05, 브랜치 `web/storage-guards`) — 수정 완료, main 병합(`2f33138`)

- 회귀: [24-blocked-storage.mjs](../web/tests/scenarios/24-blocked-storage.mjs)가 localStorage getter·`getItem`·`setItem` 차단을 차례로 주입하고, 매번 IndexedDB `open`도 실패시킨 채 문서 표시·새 문서·그리기·`.ugu` 다운로드·미처리 예외 없음을 확인한다.
- 수정 전 결과: getter·`getItem` 차단에서 컴포넌트 초기화가 예외로 끝나 셸이 문서를 띄우지 못했다(4건 실패). `setItem` 차단은 이미 통과했다.
- 수정(`13d1b56`): `readPreference`·`writePreference` 하나로 모든 preference 접근(도구 설정, 최근 색, 그리는 중 애니메이션)을 모았다. 공용 엔진 변경 없음.
- 수정 후 결과(Chromium, 로컬 엔진 빌드): `blocked-storage` 15/15, 전체 browser suite 24개 시나리오 통과, `npm run check` 0 오류·0 경고.

#### R04·R05·W-04·W-05 진행 (2026-10-05, 브랜치 `web/recovery-and-replace`) — 수정 완료, main 병합(`04aa1d6`)

- 회귀: [25-recovery-sessions.mjs](../web/tests/scenarios/25-recovery-sessions.mjs)(답하지 않은 offer 보존, 버리기 범위, 실패한 복원, 두 탭), [26-document-replace.mjs](../web/tests/scenarios/26-document-replace.mjs)(새 문서·열기·이탈 전 확인, 쓰기가 지연된 동안 문서를 교체한 뒤의 복구본). 하니스에 `recoveryRecords`·`seedRecoveryRecord`·`installRecoveryDelay`(IndexedDB open 지연)·`acceptReplacePrompts`를 추가했다.
- 수정 전 결과: 25에서 5건, 26에서 6건 실패. 새 세션 snapshot이 offer를 덮었고, 버리기가 현재 세션 snapshot까지 지웠고, 실패한 복원이 배너를 없앴고, 둘째 탭이 첫 탭의 살아 있는 문서를 제안하고 덮었고, 교체·이탈 확인이 없었고, 쓰기가 지연된 동안 교체한 새 문서 대신 이전 문서가 복구본으로 남았다.
- 수정(`58e6aac`, 웹 셸만 변경):
  - 복구 기록을 문서 id별 키로 저장하고 기록에 쓴 세션을 남긴다. 탭은 살아 있는 동안 세션 Web Lock을 쥐며, offer는 lock이 없는(끝난) 세션의 기록만 최신순으로 보인다. 예전 `slot` 기록은 세션 없는 기록으로 그대로 제안된다. Web Locks가 없는 브라우저에서는 다른 세션 기록을 모두 제안한다.
  - 자동저장은 편집 큐 안에서 문서 id·revision·이름·bytes를 함께 캡처하고, 저장 여부를 (문서 id, revision)으로 판단한다. 기록 쓰기·삭제는 한 줄로 순서대로 실행하고, 놓아준 문서의 늦은 쓰기는 건너뛴다.
  - 복원은 bytes 사본을 열고, 열기가 성공한 뒤에만 offer를 소비하고 기록을 현재 세션 소유로 바꾼다. 복원된 문서는 그 기록 키를 그대로 문서 id로 쓴다. 버리기는 그 기록만 지운다.
  - 마지막 다운로드 이후 바뀐 문서를 새 문서·열기·복원으로 교체하면 `confirm`으로 묻고, 수락하면 교체 성공 뒤 이전 문서의 기록을 지운다. 바뀐 문서가 있으면 `beforeunload`로 묻는다. 다운로드는 "Download of … started"로만 표시하고 디스크 저장으로 표시하지 않는다. 다만 교체 확인 기준으로는 다운로드 시작을 저장으로 본다(데스크톱의 저장 뒤와 같음). 복원된 문서는 다운로드 전까지 바뀐 문서로 본다.
  - 그린 문서를 교체하는 기존 시나리오 12개(01·02·04·05·08·09·15·16·17·19·21·22)는 확인 대화상자를 수락하게 했다.
- 수정 후 결과(Chromium, 로컬 엔진 빌드): 25·26 통과, 전체 browser suite 26개 시나리오 통과, `npm run check` 0 오류·0 경고.
- 2026-10-05 main에 fast-forward 병합·푸시(`04aa1d6`). 이 변경을 담은 main CI 결과는 다음 세션에서 확인한다(문서 커밋 푸시로 이전 run은 취소될 수 있음).
- 남은 것: R03의 느린 엔진 매트릭스(pen-up·undo·레이어 변경 직후 저장, pending text/transform), 끝난 세션 기록이 쌓이는 상한(지금은 사용자가 하나씩 버림), Web Locks 없는 브라우저에서의 두 탭 구분.

**CI 시간 단축 (2026-10-06, 사용자 결정)**: macOS 러너 대기가 CI 시간 대부분이었다.

- 경로 조건: 첫 `changes` 잡이 PR의 바뀐 경로를 분류한다. 문서(`docs/`, `release-notes/`, README류)만 → 형식 검사만, `web/`·웹 도구 → Web shell·Wasm, 그 밖 → 전부. main 푸시는 항상 전부 돈다. Quality gate는 `changes` 성공을 요구하고 그 위에서만 "건너뜀"을 통과로 본다.
- macOS 제외: macOS ASan + UBSan, macOS Coverage(라인 70% 게이트 포함), macOS package 잡을 뺐다. 형식 검사(clang-format 22.1.3 pipx 고정, 저장소 전체 통과 확인)와 번역 검사는 Linux로, wasm·퍼저·Clang-Tidy는 새 `linux-debug`·`linux-fuzzing` 프리셋으로 Ubuntu 24.04에 옮겼다(LLVM 22는 apt.llvm.org). 잃는 것: macOS 빌드·패키지 회귀, Windows Debug 외 sanitizer 실행, 커버리지 게이트. `release.yml`의 macOS 패키징은 그대로 둔다.
- 로컬 확인(WSL Ubuntu 26.04, Qt 6.11.1): 앱·테스트 전체가 clang 22 경고=오류로 빌드, Linux 네이티브 probe의 Wave.ugu 프레임·직렬화 digest가 wasm 엔진과 일치, 퍼저 4종 ASan+UBSan 10초씩 무사, Clang-Tidy 183개 파일 경고 없음(59초). `.mm` 소스는 Linux 컴파일 DB에 없어 tidy 대상에서 빠진다.
- CMake 4 + clang이 C++ 모듈 스캔용 `@modmap` 인자를 컴파일 DB에 넣어, 기본 빌드에서 빠진 도구의 tidy가 실패했다. 모듈을 쓰지 않으므로 `CMAKE_CXX_SCAN_FOR_MODULES`를 껐다.

**웹 레이어 선택 직후 버튼이 이전 레이어에 적용됨 — 해결 (2026-10-06)**: 레이어 패널의 버튼(지우기·삭제·아래로 병합·복제·이동·블렌드 등), 텍스트 적용, 레이어 우글거림은 엔진이 보낸 레이어 목록의 `active`로 대상을 정했다. 행을 클릭한 뒤 엔진이 답하기 전에 버튼을 누르면 이전에 선택돼 있던 레이어 id로 명령이 나갔다(엔진 큐 순서와 무관하게 id를 클릭 시점에 잡음). CI 시나리오 20(clipboard-and-layers)의 간헐 타임아웃이 이것이었다: 클릭한 `Layer 1` 대신 `Layer 2 copy`를 지웠고, 같은 잉크가 아래 `Layer 2`에 있어 픽셀이 줄지 않았다.

- 수정: 셸이 클릭한 레이어를 엔진 확인 전에도 활성으로 보여 주고(`pendingActiveLayer`), 패널·텍스트·우글거림 패널이 그 값을 쓴다. 활성화 요청이 끝나면 엔진이 보낸 실제 상태로 돌아간다.
- 회귀: 시나리오 20이 행 클릭부터 Clear까지 워커 응답을 300ms 늦춘다. 수정 전 2/2 실패(타임아웃), 수정 후 3/3 통과. 전체 browser suite 26개 시나리오(236 검사) 통과, `npm run check` 0 오류·0 경고(Windows, Chromium, 로컬 엔진).

버전 2.2.11/2.2.12에 어느 범위를 넣을지는 이 문서에서 확정하지 않는다. 변경량과 회귀 위험을 확인한 뒤 배포 단위를 정한다.

공통 완료 규칙:

- 구현 commit, 수정 전 실패/수정 후 통과하는 회귀, 실제 실행 명령·환경·결과를 기록한다. 단순 코드 읽기나 문서 체크박스로 완료 처리하지 않는다.
- 공용 엔진 수정은 관련 native suite와 WASM build/parity·실엔진 browser 회귀를 모두 확인한다. desktop-only 변경은 관련 UI suite와 해당 OS 실동작을 확인한다.
- 파일/복구 변경은 실패·취소·문서 교체·undo를 포함하고, 패키징 변경은 설치 artifact에서 검증한다.
- 실기기·업데이트·접근성·법률 검토가 남으면 구현 완료와 별도 상태로 둔다. 다른 플랫폼 성공을 대체 근거로 쓰지 않는다.
- 해결 시 기존 ID를 지우지 않고 상태·근거 commit·검증 날짜를 갱신한다. 새 측정은 이전 조건/원자료를 보존하고 현재 결론만 갱신한다.

## 10. 증거와 문서 역할

이 문서는 과거의 데스크톱·웹 검토, 성능 후속, 웹 타당성 조사, Android 계획에서 현재도 필요한 근거·결정·완료 조건을 모두 옮긴 단일 기준 문서다. 통합 전 문서의 중복·오래된 상태 설명·당시 일정 추정은 제거했다. 과거 테스트 기록은 다시 실행한 결과로 읽지 않는다.

[review-evidence-2026-09-08](review-evidence-2026-09-08/)의 probe·로그·메타데이터·캡처는 당시 재현의 원자료이므로 보존한다. 낡은 소스 추출 probe가 현재 함수에 맞지 않는다면 원본 증거를 고쳐 쓰지 말고 새 재현을 별도로 추가한다.

root README(ko/en/ja)는 사용자 안내, BUILDING/CONTRIBUTING은 빌드·기여 절차, SECURITY/THIRD_PARTY_NOTICES는 정책·고지, release-notes는 버전별 불변 이력이다. 이 성격이 다른 문서들은 통합 검토에 복사하거나 삭제하지 않는다.

## 11. Rust 3.0 재작성 — M0 완료(펜 제외), M1·M2·M3 완료, M4 진행

계획은 [RUST_WINDOWS_PORT_PLAN.md](RUST_WINDOWS_PORT_PLAN.md), 범위·결정은 [rust/scope.md](rust/scope.md), 측정·실증 근거는 [rust/m0-evidence.md](rust/m0-evidence.md)에 둔다. M0~M3은 main에 합쳤고, 다음 단계는 새 브랜치에서 한다. 이 절은 진행 상태만 적는다.

2026-10-07 기준:

| M0 작업 | 상태 |
|---|---|
| 1일차 범위 고정 | 완료. 도킹은 3.0 포함으로 확정. Animated WebP는 내보내기 인코딩만 `libwebp`, 이미지 디코드는 `image-webp`로 확정 |
| 2일차 workspace·CI | 완료. `apps/ugurugu`, `crates/ugu-core`·`ugu-render`·`ugu-win`, `tools/latency-probe`. wgpu는 DX12만 빌드. CI `Rust Windows` 잡은 draft PR #18에서 처음 통과 |
| 3~4일차 기준선 | 완료. 입력→표시, fixture ①~⑤ 렌더에 더해 `tools/baseline_probe.py`로 UI 스레드 CPU·점유, 저장·열기, 취소, RAM, 재생 fps, 내보내기 중 UI 반응을 밖에서 쟀다. 칸당 UI CPU p95 2.2~3.8ms·점유 p95 7.5~10.9ms, pen-up p95 18~21ms, 저장 20~286ms(작업자 스레드), 열기 118~561ms(UI 스레드 동기), 취소 p95 0.6~3.7s, working set 1GiB 예산의 절반 아래, 재생은 ③(처음 7~16초)을 빼면 처음부터 25fps, 내보내기 중 UI 응답 p95 0.4ms 이하. m0-evidence 5절 |
| 5~7일차 입력 | 마우스로 부분 완료. `WM_POINTER` subclass와 coalesced 마우스 이동 복원(약 500/500). 펜 장치가 없어 필압·hover·barrel·WinTab은 미검증 |
| 13일차 일부 (presenter) | 완료. 대기 → 최신 입력 → 렌더 → present, UI/렌더 스레드 분리, DXGI 통계 계측. 워밍업 후 같은 표시 방식(Independent Flip)에서 입력→표시 p50 5.9~6.4 / p95 8.5~9.0ms로 C++(9.6~10.9 / 12.9~13.4ms)보다 약 4ms 빠르다. 이전의 "약 1ms 느림"은 측정 절차 탓이었다 |
| 13일차 일부 (캔버스 표시) | 완료. 창 전체 swapchain 하나에 Vello CPU 캔버스를 그리고 그 위에 egui를 그린다(자식 창 기각, ADR은 m0-evidence 4절). 화면 확인 중 마우스 이동 복원 버그를 찾아 고쳤다 |
| 10~11일차 렌더러 비교 | 측정 완료(`tools/render-bench`, 2048² 교대 3회). Vello CPU가 1스레드에서 래스터화 3.4배, 8스레드에서 약 24배 빠르고 메모리는 비슷하다. tiny-skia와의 차이는 가장자리 안티에일리어싱뿐이다. Vello CPU 채택, Vello GPU 보류로 확정. Vello GPU는 래스터화가 CPU 8스레드보다 5배 느려(전처리가 1스레드) 재생 경로에도 쓰지 않는다 |
| 8~9일차 IME·접근성 | 완료. 한국어·일본어 IME를 100/125/150/200%에서 자동 시험해 통과(캔버스 포커스·CJK fallback 기준선 버그 수정). AccessKit으로 UI Automation 트리 노출(텍스트 칸·버튼, 포커스·입력·Invoke 동작). IME 후보창을 커서 바로 아래로 옮김. egui TextEdit는 SetValue 미지원. Narrator 청취 검증은 범위에서 뺌(사용자 결정) |
| 12일차 ADR | 완료. 레이어 연산 enum·병합(isolated section, 불투명도·모션 차이 허용)·채우기(확정 coverage)·선택 의미와 `.ugurugu` 스키마 1. `crates/ugu-core` 의미 시험 7개. [adr-operations-and-format.md](rust/adr-operations-and-format.md) |
| 13~14일차 | 완료. 장치 손실 복구(RemoveDevice로 3회 시험, 캔버스 보존, 0.4~0.9초), WARP fallback과 안내, 4K 최대화 창 입력→표시 p50 5.8 / p95 8.9ms(2560 창과 같음), WARP는 p50 59~76ms, 오프라인 진입점 확인. 실제 TDR은 M3, RDP는 M6. m0-evidence 8절 |
| 15일차 마무리 | 완료. 기술 조합·버전 고정 정책·M0 종료 조건 대비·위험표·M1 분해(10단계). 펜만 미충족으로 M6 전 gate에 이월. [m0-close.md](rust/m0-close.md) |
| M1 도메인·새 저장 | 완료(`rust/m1`). 레이어 트리·저장소·검증, 원자적 커밋, undo/redo·macro·dirty 판정, `.ugurugu` 쓰기·읽기·거부·안전 저장(`ReplaceFileW`), 구형 판별, headless 도구. 커밋 0.2ms(연산 상한), `.ugurugu`는 2.2.13보다 1/5~1/9 크기에 저장·열기 모두 빠름. m0-evidence 9절 |

| M2 첫 드로잉 완주 | 완료, main에 합침(PR #20, CI 통과). 분해·결정·진행은 [rust/m2-plan.md](rust/m2-plan.md). Classic 모션(C++ 골든 값과 비트 단위 일치), 획 합집합 윤곽, Vello CPU 문서 렌더러, `ugu-session`, 레이어 분할 캐시와 pen-up 증분, 진행 중 획, 확대·이동 표시, 제품 UI, 타임라인·재생, 파일, PNG. 입력→표시는 M1과 같은 수준, pen-up p95 1.1ms(③), 재생은 ③④도 2초 안에 25fps. m0-evidence 10절 |
| M3 렌더·스케줄러 완성 | 완료, main에 합침(PR #21, CI 통과). 분해·결정·진행은 [rust/m3-plan.md](rust/m3-plan.md)(M3-1~M3-14). 2026-10-07 기준 M3-1~M3-14 완료: 레이어 표면 + 계획 합성(그룹·클리핑·합성 모드), 128px 타일, 레이어별 revision 캐시, 계획 기반 편집 분할(화면이 전체 렌더와 바이트 단위로 같음, pen-up p95 3ms 이하), 우선순위 작업자와 중단(p95 44ms 이하), 축소 재생, 메모리 예산과 예산 초과 시 단계별 렌더. GUI에서 유휴·최소화 렌더·업로드 0회, ②p·⑤p 재생이 모든 프레임을 캐시하며 working set 1GiB 안(951·964MiB), 앱 pen-up p95 1.7ms를 확인했다(축소 재생이 첫 프레임에 멈추던 버그와 재생 중 메모리 초과를 고침). M3-9 완료: 그룹 명령과 2.2.13 기준 UI(테마·아이콘·배치·레이어 도크·썸네일·Fluent 번역, 한국어·일본어는 2.2.13 번역 재사용), 레이어 도크 GUI 시험·IME·UIA 재시험 통과(시험에서 찾은 포커스·단축키·접근성 이름 문제 수정). M3-10 완료: 20,000·50,000·100,000 연산을 재고 상한 20,000을 유지하기로 결정. M3-11 완료: 2560 창이 승격되지 않던 것은 계속 그리는 다른 창이 오버레이 평면을 쓰고 있었기 때문이고, 오후에 기준선이 한 프레임 느렸던 것은 주 모니터가 60Hz였기 때문(160Hz에서 M2와 같은 수준). M3-12 완료: 실제 TDR 12회 복구(1.8~4.3초), 흰 창·썸네일 손실·멈추는 새 스왑체인을 고침. M3-13 완료: GPU 가속은 하지 않음(병목은 CPU 윤곽·인코딩). 재생은 화면 크기에 맞춘 표본 간격과 코어 수 − 1 작업자로 개선. M3-14 완료: 획 윤곽을 원과 띠의 합집합 그대로 윤곽 하나로 이어 ②p 8스레드 프레임 79 → 40ms, 앱 첫 바퀴 1.35초. 종료 측정은 m0-evidence 11절, 대비는 m3-plan 9절. 미달은 ②p 30fps 처리 능력 하나(25fps 수준)이고, 사용자 결정(2026-10-07)으로 M3에서 받아들여 M4 첫 항목으로 넘김. 사용자 결정: 메모리보다 성능 우선, 벤치마크는 작업자 8스레드 기준. 도킹(도크 이동·탭·떼어 띄우기·배치 저장)은 계획대로 M5. |

다음 작업:

1. M4 편집 기능 완성(브랜치 `rust/m4`): 분해·결정·진행은 [rust/m4-plan.md](rust/m4-plan.md)(M4-1~M4-15). 2026-10-08 기준 M4-1~M4-6 완료: 윤곽 경로 재사용(②p 8스레드 39 → 35.5ms, 앱 첫 바퀴 1.24s, 33ms는 M4-6에서 다시), GUI 재측정과 진행 중 획 버퍼 재사용(⑤p pen-up CPU 6.3 → 4.0ms), 선택 clip·채우기(2.2.13 화소 규칙)·선택 지우기, 선택 변형·이미지 배치(원본 ⑤ 1스레드 540ms, 2.2.13 951~967ms), 자르기(끊지 않고 옮겨 그림)·리사이즈(2.2.13 리샘플 규칙, 버퍼 보관)로 원본 ⑤에 둘 다 넣은 문서가 8스레드 60.4ms, 캐시 예산은 설치 메모리 1/4와 여유의 절반 중 작은 것(원본 ⑤ 100% 재생 10.6 → 25fps, 2737MiB, 다른 프로그램이 메모리를 쓰면 줄어 페이징 없음). M4-6(브러시) 완료: 필드·프리셋 20개·렌더(사각 끝 마커, 에어브러시, 스프레이, 지우개 3종), dab은 전용 래스터(원본 ② 8스레드 56.4 → 42.2ms, ⑤ 106.5 → 89.9ms), 진행 중 획·pen-up도 같은 래스터, 앱 프리셋 선택(GUI 확인). 원본 ②의 33ms는 일 총량 하한 34ms로 미달로 닫음(사용자 결정). 사용자 요청으로 M4 중간에 2.2.13과 headless 렌더를 비교했다(m0-evidence 12절: 1스레드 1.5~34배, 8스레드 1.2~26배 빠름, 1스레드 메모리 2~3.5배). 다음은 M4-7(모션 전체). 사용자 결정: mimalloc은 측정 뒤 넣지 않음, RAM 1GiB는 참고값이고 예산은 PC 메모리에 맞춤(M4-5b), 2.2.13과의 비교 측정은 M4를 마친 뒤 종료 측정(M4-15)에서 같은 조건으로 한꺼번에. 사용자 결정(2026-10-08): 확장자를 버전 없는 `.ugurugu`로 바꿈(`.ugu2`의 "2"가 2.x 앱으로 읽힘). 사용자 결정(2026-10-07): 회전도 대기 편집으로 통일, 복사는 클립보드만·붙여넣기는 새 레이어와 대기 편집, 다른 앱 이미지 붙여넣기 지원, 브러시 필드는 `.ugurugu` 스키마 1을 넓힘(M2·M3 빌드 파일은 열지 않음), 모션 Smooth·Stepped·끊어진 선은 C++ 수식을 골든 값으로 대조, 문자는 Parley·Fontique·Skrifa로 시작해 DirectWrite와 비교.
2. 펜 장치를 확보하면 펜 경로 실증(M6 전 필수 gate).

측정 재현: PresentMon 2.6.0은 관리자 권한이 필요하고, 앱보다 먼저 시작해야 하며, `--no_track_input`이 필요하다(입력 추적이 지연을 늘림). 창 배치 직후에는 DWM이 합성하므로 `--warmup 150`을 넣는다. `misses`가 0이 아닌 실행은 버린다. C++ 앱은 재생이 켜진 채 시작하므로 `P`로 정지한 뒤 잰다. 입력 주입은 `SetCursorPos`가 아니라 `SendInput`으로 한다. C++ 앱 실행에는 PATH에 Qt `bin`과 설치본의 `velopack_libc.dll` 폴더가 필요하고, `UGURUGU_INSTANCE_LOCK_PATH`·`UGURUGU_RECOVERY_PATH`로 설치본과 분리한다. 예: `latency-probe --exe <Ugurugu.exe> --presentmon <PresentMon.exe> --steps 100 --warmup 150 --key P`.
