# Ugurugu 통합 검토·개선 계획

정리일: 2026-09-18 · 제품: **2.2.10** · 코드 기준: `9172ac668e1949496e1cc1df2759b80bdd327839` 이후 현재 작업 트리

이 문서는 데스크톱·공용 엔진·웹의 검토 결과, 성능 후속 작업, Android 이식 계획을 합친 **현재 상태의 단일 기준 문서**다. 과거 문서의 발견 번호 `R01–R14`, `D01–D25`는 추적용으로 유지한다. 중복 번호를 독립 결함 수로 합산하지 않는다.

문서 통합 뒤 P1 개선을 시작했다. D04/R01의 bounded decode와 중복 검증 축소, D16의 Sparkle 수정 버전 고정, D05의 최종 저장 경로 확인, D06의 격리된 Windows 패키지 smoke는 현재 작업 트리에 반영했지만, 네이티브·WASM·실제 설치 수용 검증은 아직 끝나지 않았다. 역사적 측정과 테스트는 실행 당시의 조건에만 유효하다.

## 1. 핵심 판단

- 공용 문서 모델, 트랜잭션·원자적 저장, 증분 렌더, 취소·세대 관리, 자원 예산과 CI는 유지할 기반이다. 전면 재작성보다 경계별 회귀 테스트와 수정이 우선이다.
- 먼저 해결할 것은 **압축 해제 상한, 업데이트 의존성 보안, 변환·표시 정확성, 저장·복구 경계**다. 대규모 리팩터링이나 신규 플랫폼 UI보다 앞에 둔다.
- 9월 8일 보고서의 네이티브 항목 7개는 관련 코드가 남아 있다. 웹 수동 저장의 큐 우회는 이후 수정됐지만, 자동저장 큐·문서 세대·미저장 문서 교체 문제는 남아 있다.
- 성능 개선 두 건은 이미 구현됐다. 이를 다시 구현할 일이 아니라, 정량 A/B 측정과 실기기 검증을 마쳐야 한다.
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
- 남음: Qt가 있는 환경에서 native suite와 WASM build/parity를 통과시키고, 퍼저 corpus·peak memory·decode 횟수 비교를 기록한다. 이 검증 전에는 해결로 닫지 않는다.

### D16 · P1 · Sparkle 보안 후속 — 검증 대기

기존 2.9.4는 공식 권고 GHSA-3x7w-j75x-ppq5의 영향 범위 `<=2.9.5`에 포함된다. 시스템 권한 installer에서 경로 검증과 이동 사이의 symlink 교체와 관련된 **로컬·높은 공격 복잡도** 문제이며, Ugurugu의 모든 설치가 공격 가능하다고 단정하지 않는다. [공식 보안 권고](https://github.com/sparkle-project/Sparkle/security/advisories/GHSA-3x7w-j75x-ppq5).

- 구현됨: 같은 2.9 계열의 수정 버전 2.9.6으로 올리고, 공식 배포본에서 직접 계산한 SHA-256 `52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192`를 CMake에 고정했다. [공식 2.9.6 릴리스](https://github.com/sparkle-project/Sparkle/releases/tag/2.9.6).
- 남음: macOS에서 실제 패키지의 framework 버전과 서명, 신규 설치, 2.9.4 포함 기존 앱에서 업데이트, 관리자 권한 경로를 검증한다. delta 비활성화만으로 이 installer 문제를 닫지 않는다.

### D05 · P1 · 최종 저장 이름의 덮어쓰기 확인 — 검증 대기

근거: [SavePathDialog.cpp](../src/ui/SavePathDialog.cpp), [MainWindow.cpp](../src/ui/MainWindow.cpp)의 저장, [MainWindowExport.cpp](../src/ui/MainWindowExport.cpp), [MainWindowSettings.cpp](../src/ui/MainWindowSettings.cpp).

기존에는 대화상자가 반환한 이름을 확인한 **뒤** 확장자를 정규화했다. 함수는 접미사가 없을 때뿐 아니라 기대 접미사와 다를 때도 확장자를 덧붙였으므로, 대화상자가 확장자를 자동 보정하지 않는 경로에서는 확인 대상과 실제 교체 파일이 달라질 수 있었다. 모든 OS 기본 대화상자에서 재현됐다는 뜻은 아니다.

- 구현됨: 공용 저장 대화상자가 형식별 기본 접미사를 선택 전에 설정한다. 알려진 이미지 접미사는 선택 필터보다 우선해 PNG/JPEG 명시를 보존하고, 알 수 없는 접미사는 선택 형식의 접미사를 덧붙인다. 정규화로 경로가 바뀌고 그 최종 대상이 이미 있으면 별도 확인한다. 프로젝트·PNG/JPEG·GIF/WebP·WWP 프리셋이 같은 경로를 사용하며 기존 `QSaveFile` 저장은 유지한다. [Qt `defaultSuffix`](https://doc.qt.io/qt-6/qfiledialog.html#defaultSuffix-prop), [Qt overwrite 확인 기본값](https://doc.qt.io/qt-6/qfiledialog.html#Option-enum).
- 회귀 추가: 기본 접미사, 확장자 없음·다른 접미사·대소문자가 다른 같은 접미사, 정규화 뒤 확인 취소 시 기존 파일 보존, 이미 확인된 같은 이름 경로를 검사한다.
- 남음: Qt가 있는 환경에서 offscreen/non-native 회귀를 실행하고 Windows·macOS native 대화상자에서 접미사 없음·다른 접미사·취소를 수용 검증한다. 이 검증 전에는 해결로 닫지 않는다.

### R03 · P1 · 웹 저장 순서 — 부분 해결

현재 [App.svelte](../web/src/App.svelte)의 `downloadDocument`는 `enqueueExclusive` 안에서 직렬화한다. 따라서 예전의 **수동 저장이 편집 큐를 우회한다**는 발견은 해당 경로에서 수정됐다. 텍스트 미리보기의 적용/취소 안내도 추가됐다.

반면 autosave host의 `serialize`는 여전히 `engine.serialize()`를 직접 호출한다. 큐에 대기 중인 편집과 복구 snapshot의 순서, 문서 identity·이름·revision의 원자적인 캡처는 남은 과제다. `ready: !drawing`은 대기 중 명령이 없다는 보장이 아니다.

- 완료: 느린 엔진에서 pen-up·undo·레이어 변경 직후 수동/자동 저장, 저장 직후 열기, pending text/transform 각각을 시험한다. 수동 저장 수정은 유지하고 자동저장도 동일한 세션 순서 계약을 따른다.

### R04 · P1 · 웹 자동복구의 문서 세대 — 미해결

근거: [AutosaveController.svelte.ts](../web/src/lib/AutosaveController.svelte.ts), [RecoveryStore.ts](../web/src/lib/RecoveryStore.ts). `reset()`은 `savedRevision`만 0으로 바꾸며 진행 중 snapshot을 무효화하지 않는다. 저장 뒤 이전 revision이 새 문서의 저장 상태를 덮을 수 있다. 이름은 serialize 이후에 읽으므로 문서와 이름의 소속도 함께 고정해야 한다.

과거 소스 함수 재현에서는 A의 revision 1 저장을 지연한 뒤 B로 교체하자, B의 revision 1을 저장된 것으로 오인해 snapshot을 생략했다. 저장소는 같은 origin의 단일 `slot`이다.

- 조치: session/project identity·generation·revision을 캡처한다. await 이후 상태 채택뿐 아니라 오래된 IDB write 자체가 현 복구본을 대체하지 못하도록 저장소 키/채택 규칙을 설계한다.
- 완료: serialize 지연, IDB 지연, 연속 문서 교체, 복구 폐기 중 저장 완료, 복수 탭을 시험한다. 낡은 완료가 새 문서의 저장 표시·복구본을 바꾸면 실패다.

### R05 · P1 · 웹 문서 교체 전 미저장 작업 보호 — 미해결

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

### D01 / R02 · P1 · 이미지와 선택 변환의 합성 순서 — 미해결

근거: [DocumentControllerStrokes.cpp](../src/document/DocumentControllerStrokes.cpp)의 마스크 없는 `duplicateStrokes`·`transformStrokes`, [CanvasWidget.cpp](../src/ui/CanvasWidget.cpp)와 [CanvasWidgetSelection.cpp](../src/ui/CanvasWidgetSelection.cpp)의 세션 누적. `delta * existing`이 자산→문서 배치와 문서 좌표 delta의 순서를 뒤집는다. Qt는 곱의 왼쪽 변환을 먼저 적용한다. [Qt 변환 합성](https://doc.qt.io/qt-6/qtransform.html#combining-transforms).

- 조치: 자산·문서·뷰 좌표 계약을 명시하고 코어와 UI의 합성 경로를 함께 대조한다. 마스크 분기는 별도 `PixelSelectionOp` 경로이므로 같은 코드로 가정하지 않는다.
- 완료: 축소·중앙 배치된 이미지에 이동→회전→뒤집기를 연속 적용하고 독립적인 점 매핑 oracle·픽셀 결과·undo/redo와 비교한다. 항등 행렬/순수 이동만으로는 충분하지 않다. 저장된 모든 행렬을 일괄 뒤집지 않는다.

### D02 · P1 · 우글거림 OFF에서 pen-up 프리뷰 승격 — 미해결

근거: [CanvasWidgetTools.cpp](../src/ui/CanvasWidgetTools.cpp)의 `endStroke`는 `activeStrokePreview`에 원본 controller 문서를 넘긴다. 다른 표시 경로는 문서·레이어 wobble을 제거한 `displayDocument()`를 사용한다. 결과가 정확한 프레임으로 캐시에 승격되는 소스 경로를 확인했다. 이번에 실제 화면 재현은 하지 않았다.

- 완료: 문서·레이어 wobble ON/OFF 각각에서 pen-up 결과가 `renderScaled(displayDocument(), …)`와 같고, 캐시 재사용·재생 전환 후에도 동일하다. 수정은 표시 문서 계약을 따르게 한다.

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
| R08 · P2 · 미해결 | [Shortcuts.ts](../web/src/lib/Shortcuts.ts)의 range 방향키·button Enter가 앱 명령으로 처리됨. 과거 소스 함수 재현. 현재 App의 Space 버튼 보호만으로 해결되지 않음 | Tab으로 range/Home/End/방향키, button Enter/Space를 기본 의미로 사용 |
| R09 · P2 · 미해결 | [App.svelte](../web/src/App.svelte)의 `animateWhileDrawing` 초기화가 localStorage를 보호 없이 읽음. 안전한 preference reader와 세션 기본값 사용 | getter/getItem/setItem/IDB 각각 실패해도 새 문서·다운로드 가능 |
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

### D08 · P2 · 프레임 warmup의 동시 임시 표면 — 조사·예산 공백

[CanvasWidget.cpp](../src/ui/CanvasWidget.cpp)의 최대 8 worker와 [CanvasWidgetPreview.cpp](../src/ui/CanvasWidgetPreview.cpp)의 동시 렌더에 대해 [PreviewRenderPolicy.cpp](../src/render/PreviewRenderPolicy.cpp)의 임시 비용 계산은 동시 작업 전체를 반영하지 않는다. 보존 표면 예산 테스트가 프로세스 peak를 보장하지 않는다.

조치는 worker별 working set, 고정·보존 표면, 취소 중 작업의 잔여 수명을 포함한 동시성 제한이다. 예전의 “추가 1.5–2.5GiB”는 실제 관측값이 아니므로 수용 기준으로 쓰지 않는다. 정확성·재생 재개 시간과 함께 측정한다.

### D14 · P2 · 스포이드의 전체 동기 렌더 — 조사

[CanvasWidgetTools.cpp](../src/ui/CanvasWidgetTools.cpp)의 샘플링은 UI thread에서 전체 해상도 프레임을 렌더할 수 있다. 문서 크기별 첫 pick 지연을 먼저 잰다. 정확한 영역 렌더/동일 revision 캐시를 검토하되 축소 preview 픽셀로 대체하면 색 의미가 달라질 수 있다. “클릭마다 수 초”는 실측 전 확정하지 않는다.

### R11 및 웹 메모리 정책 — 미해결

[MemoryPolicy.ts](../web/src/lib/MemoryPolicy.ts)의 신규/resize 최대 변은 desktop 2048·mobile 1024이나 파일 열기는 바이트 크기 중심이며, 공용 reader는 4096까지 허용한다. [App.svelte](../web/src/App.svelte)의 파일 선택은 `arrayBuffer()` 후 크기를 검사한다.

- 읽기 전 `File.size`, 문서 채택 전 크기·decoded raster·mask·렌더/GPU 표면 비용을 검사한다. 큰 파일 허용이 의도라면 별도의 import 정책·축소 제안을 정의한다. 자동 축소 후 원본 덮어쓰기는 금지한다.
- WASM 최대 heap 512MB는 JS·GPU·브라우저 전체 예산이 아니다. CPU 표면과 GPU texture 중복·export·serialize의 동시 peak를 측정한다.
- **정정:** 공용 `DocumentUndoStack`에는 현재 192MiB resident 기준과 byte 기반 정리가 있다. 다만 가장 최근 항목 하나는 soft exceed가 가능하다. 웹 `ugu_set_undo_limit`은 개수만 설정하므로 웹 프로파일의 MiB 정책을 연결/관측하는 작업이 남는다. “엔진에 byte 예산 없음”은 코드 주석에도 남은 오래된 설명이다.

### D21 · P3 · UI 갱신 비용 — 조사

색 기록의 256버튼 일괄 stylesheet 갱신, [Theme.cpp](../src/ui/Theme.cpp)의 강조색 변경 시 폰트·스타일 재등록, [TimelineBar.cpp](../src/ui/TimelineBar.cpp)의 숫자 입력 도중 프레임 캐시 무효화가 대상이다. 일회성 초기화 분리, 변경된 항목만 갱신, 숫자 편집 완료 시 적용을 검토한다. 사용자 입력 지연을 측정하기 전에 전체 위젯 재작성으로 범위를 늘리지 않는다.

### 추가 프로파일 후보와 측정 계약

AA/가변 필압 긴 획의 반복 래스터, 빈 레이어 표면 할당, 선택 clip path 재구성, 커버리지/합성 계획 반복, fill의 전체 캔버스 순회, 획별 임시 할당을 후보로 유지한다. O(n²) 또는 주 병목이라는 판단은 primitive 처리량·시간·할당 프로파일로 확인한다.

저장소 내 native benchmark를 먼저 마련한다. 외부 fixture 경로+SHA와 빌드/기기/OS/표시 조건을 받고 JSON 결과를 남긴다. 개인 문서는 동의 없이 커밋하지 않는다. 조건별 최소 50회·실행 순서 교차, cold/warm·open/undo 직후·immediate/idle·회전 0/5도·software/GPU·wobble ON/OFF·빈/실제 문서를 구분한다. p50/p95/max, UI/process CPU, worker·취소 지연·upload bytes·peak RSS를 기록한다.

웹의 과거 첫 획 1024² 2.7초 / 2048² 5.5초와 이후 시작 p95 0.3ms·commit p95 2ms는 당시 2,000획 조건의 기록이다. 현재 성능 보장이 아니다. 자동복구 serialize와 GIF의 Worker 점유도 별도 측정한다. 실제 펜 event→presentation, Metal trace, Windows 입력은 미검증이다. 전체 GPU 이관이나 파일 포맷 변경은 현재 우선 작업이 아니다.

## 6. 배포·플랫폼·유지보수

| ID·우선순위·상태 | 남은 일 | 완료 기준 |
|---|---|---|
| D06 · P1 · 부분 해결 | Windows 설치본에 [qt.conf](../resources/windows/qt.conf)를 두어 plugin 기준을 실행 파일 디렉터리로 제한한다. Qt는 원래 실행 파일 옆 경로뿐 아니라 설치 prefix도 탐색하므로 환경변수 정리만으로는 격리가 완전하지 않다. [Qt plugin deployment](https://doc.qt.io/qt-6/deployment-plugins.html), [Using `qt.conf`](https://doc.qt.io/qt-6/qt-conf.html). [TestWindowsPackage.ps1](../tests/TestWindowsPackage.ps1)은 PATH를 Windows 시스템 디렉터리만으로 다시 만들고 Qt/QML plugin 환경변수를 제거한 뒤 실제 `Ugurugu.exe`와 [PackageSmoke.cpp](../tests/PackageSmoke.cpp)를 실행한다. 설정·복구 경로도 임시 profile로 격리하며 CI 설치 트리와 최종 Velopack 설치본이 같은 검사를 사용한다. | 실제 앱 기동과 JPEG read-back이 성공해야 한다. `qwindows.dll` 또는 `Qt6Core.dll`을 뺀 임시 복사본은 반드시 실패한다. 실제 CI/release 성공을 확인하고, Velopack의 외부 `vcredist145-x64` prerequisite는 Qt·개발 도구가 없는 clean Windows 설치에서 별도 검증한다. |
| D09 · P2 · 미해결 | `.ugu` 파일 연결과 이미 실행 중인 앱에 두 번째 경로 전달 부재. [main.cpp](../src/main.cpp), [Info.plist.in](../resources/macos/Info.plist.in) | 새/기존 인스턴스에서 파일 열기, 공백·한글 경로·dirty 보호. `.wwpreset`은 지원 시 프로젝트와 다른 라우팅 |
| D10 · P2 · 배포 확인 필요 | 이미지 필터가 광고하는 WebP/TIFF 등과 실제 배포 plugin 구성이 일치하는지 확인. 구성만으로 최종 artifact 지원을 단정하지 않음 | Windows/macOS 설치 산출물에서 PNG/JPEG/WebP/BMP/GIF/TIFF fixture decode. 지원하지 않는 형식은 필터/안내 조정 |
| D15 · P2 · 미해결/감사 | Wawa·프리셋·복구 오류 literal, Qt 기본 번역 배포, numerus 사용 점검 | ko/en/ja 오류·파일 dialog·복수형 실제 표시. 번역 추출 100%와 사용자 경로 완전 번역을 구분 |
| D18 · P2 · 미해결 | Windows 업데이트 확인 busy 중 수동 요청이 무시될 수 있고 자동 offer가 시작 dialog와 겹침. 취소/다운로드 상태도 확인 | 자동→수동 요청, 시작 dialog 중 offer, 네트워크 실패·취소·재시도·설치 경로. 보관하지 않은 installer로 ‘나중 설치’를 약속하지 않음 |
| D22 · P3 · 구조 제안 | Canvas/App의 공유 상태, 액션 중복 계산·이름 재탐색, global filter, controller/serializer 결합 | session identity·pending edit·queue·채택 규칙 테스트 후 한 경계씩 분리. 매크로 staged document 노출은 현재 소비자 계약을 먼저 확인 |
| D23 · P3 · 정리 후보 | ToolPopover 호출 여부, 마스크 분기 조기 반환 뒤 코드, 이벤트 중복, mask/transform/선택 helper 중복 | 모든 지원 빌드/호출자를 확인하고 제거. 샘플러·렌더 품질·epsilon의 의미가 다른 코드는 모양만 보고 합치지 않음 |
| D24 · P3 · 검증 개선 | 전체 line coverage 70% 게이트 외 위험 경로 회귀, 짧은 fuzz의 corpus 보존, TSan 활용, 고정 대기·dialog fallback 개선, 플랫폼별 검사 공백 | 취소·세대 테스트가 전혀 없다고 단정하지 말고 기존 테스트별 보장 확인. 조건 기반 대기·결과 artifact·장시간 fuzz를 위험도에 따라 추가 |
| D25 · P3 · 부분 해결 | 이번 문서 통합·이력 정정은 완료. root README 세 언어 기능/단축키, BUILDING preset/target, schema 명세, release note 색인·표제 정돈은 후속 | 실제 action/CMake와 대조하고 세 언어 일치. 역사적 commit 메시지 불일치 기록은 남기되 이력 재작성은 하지 않음 |

배포 유지 사항: 의존성 URL/hash·Actions SHA 고정, main CI 통과 확인, macOS 서명·공증·Gatekeeper·rpath 감사, 테스트별 설정/복구 경로 격리, 번역·SPDX 게이트. 검증을 편하게 하려고 완화하지 않는다.

배포 후속은 아직 남아 있다. 2.2.10의 당시 CI·release 성공 기록은 실제 **2.2.9→2.2.10 updater UI·설치 완료·재실행** 검증을 대신하지 않는다. 앱 내부 세 언어 release notes dialog, Windows 실기기 설치/업데이트도 별도 확인한다.

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

1. **안전한 입력·배포:** D04/R01 bounded decode, D16 보안 버전 검토, D05 최종 경로 확인, D06 격리된 패키지 검증. 각 항목 독립 재현·회귀를 먼저 만든다.
2. **문서 정확성·보존:** D01/R02, D02, R03의 잔여 자동저장, R04·R05, R06, D07·D19·D20. 문서 identity와 pending edit 정책을 공유하되 한 번에 전부 리팩터링하지 않는다.
3. **입력·액션·접근성:** D03·D11–D13·D17, R07–R10·R12–R14, D15·D18. 실제 입력과 저장 결과를 함께 확인한다.
4. **측정 기반 성능:** 기존 두 구현의 A/B, D08·D14·D21·R11과 웹 Worker 점유. 측정 뒤에만 최적화 경계를 선택한다.
5. **제품 확장·유지보수:** 웹 선택 기능, D09·D10 artifact 점검, D22–D25의 남은 작업, Android A0–A2부터. 독립적으로 가능한 검증은 앞 단계와 병행할 수 있다.

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
