# ADR: 레이어 연산의 의미와 `.ugurugu` 파일 스키마 (M0 12일차)

상태: 확정(2026-10-07, 사용자 확인).
실행 가능한 명세: `crates/ugu-core/src/ops.rs`(연산·병합 규칙), `crates/ugu-core/src/semantics.rs`(참조 평가기와 의미 시험). 실제 렌더러(Vello CPU)는 같은 입력에서 이 시험과 같은 결과를 내야 한다.

## 1. 배경

C++ 2.2.13은 획·지우개·채우기·이미지·선택 이동·캔버스 변경·병합 경계를 모두 `Stroke` 하나와 optional 필드로 표현한다(`src/document/Document.hpp`). 렌더는 레이어마다 이 목록을 순서대로 재생한다(`src/render/engine/LayerOperationReplay.cpp`). 3.0은 구형 파일을 읽지 않으므로 이 표현을 옮기지 않는다. 대신 사용자가 보는 의미는 그대로 지키고, 연산마다 필요한 값만 가진 enum으로 다시 정의한다.

## 2. 결정: 레이어는 순서 있는 연산 목록이다

Paint 레이어의 프레임 f 결과 S(f)는 투명한 표면에서 시작해 연산을 순서대로 적용한 결과다. 각 연산은 앞선 연산의 결과에만 작용하고, 뒤에 오는 연산에는 작용하지 않는다. 모션은 획마다 프레임별로 평가하므로 S(f)는 프레임마다 다르다.

| 연산 | 의미 | C++와의 관계 |
|---|---|---|
| `Paint { stroke, clip }` | 획을 프레임의 모션만큼 움직여 그린다. `clip`은 그릴 때 있던 선택 마스크 | `Paint` + `clipMask` |
| `Erase { stroke, clip }` | 지금까지의 결과에서 획 아래를 지운다. 뒤에 그린 획은 지우지 않는다 | `Erase`(DestinationOut) |
| `Fill { coverage, color, antialias, clip }` | 만들 때 확정한 coverage 마스크를 칠한다. 모션으로 움직이지 않고, 프레임마다 다시 flood fill하지 않는다 | 2.2.13의 `fillCoverage`(새 채우기는 이미 이 방식). 구형 procedural fill은 옮기지 않는다 |
| `PlaceImage { asset, transform, sampling }` | 자산을 변환해 지금까지의 결과 위에 그린다 | `Image` |
| `TransformSelection { mask, transform, sampling, keep_source }` | 이 프레임의 지금까지 결과에서 마스크 부분을 잘라 변환해 위에 그린다 | `PixelSelection`(clearSource·drawDestination) |
| `ClearSelection { mask }` | 이 프레임의 지금까지 결과에서 마스크 부분을 지운다 | `PixelSelection`(drawDestination=false) |
| `Crop { offset, size }` | 캔버스 크기를 바꾸고 내용은 화소 그대로 offset만큼 옮긴다 | `Reframe`(Canvas) |
| `Resample { size, sampling }` | 지금까지의 내용을 새 크기로 리샘플한다 | `Reframe`(Image) |
| `Isolated(Section { ops, opacity, wobble })` | 안의 연산을 새 투명 표면에서 평가한 뒤 불투명도를 곱해 위에 그린다. 캔버스를 바꾸지 않는다 | payload 없는 `CompositeBoundary` 한 쌍을 대신한다 |

시험으로 고정한 의미:

- 지우개는 앞선 결과에만 작용한다(`an_eraser_reaches_only_what_came_before_it`).
- 선택 변형은 프레임마다 그 프레임의 결과를 옮긴다. 0프레임 래스터를 모든 프레임에 붙이지 않는다(`a_selection_transform_moves_each_frames_own_result`).
- 채우기는 선이 움직여도 coverage가 그대로다(`a_fill_keeps_its_coverage_while_lines_move`).
- 자르기는 표시 영역 변경, 리사이즈는 앞선 내용의 변환이며, 뒤의 획은 바뀐 캔버스 좌표로 그린다(`a_crop_moves_the_canvas_and_a_resample_scales_what_came_before`).

### 선택과 자동 선택

선택 자체는 문서 연산이 아니라 세션 상태(마스크)다. 선택이 문서에 남는 것은 그것을 쓰는 연산이 확정될 때뿐이다: 선택 안에 그린 획의 `clip`, 선택 이동·변형(`TransformSelection`), 삭제(`ClearSelection`), 선택 안 채우기(`Fill`의 `clip`). 자동 선택(참조 범위 3종: 현재 레이어·참조 레이어·보이는 레이어 전체)과 채우기는 클릭 시 현재 프레임의 참조 이미지에서 마스크를 계산하고, 그 결과만 저장한다. 이 계산은 세션이 불변 snapshot으로 작업자에게 맡긴다(계획 5.1절).

### 채우기의 가장자리

3.3.0부터 `behind`가 켜진 채우기(선에서 멈추는 페인트통)는 coverage ∩ clip도 대체하지 않고 가장자리와 함께 `기존 + 색 × (1 − 기존 alpha)`로 뒤에 깐다. coverage는 만들 때 선 밑으로 가운데까지 넓힌다(decisions.md 4절). 파일은 `fill` 연산의 `behind: true`(켜졌을 때만 씀)와 manifest `required`의 `"fill-behind"`다.

`antialias`가 켜진 채우기는 coverage 바로 바깥 4-이웃 화소에 색을 뒤에 깔듯 합성한다(2.2.13 `applyFillStroke`와 같은 규칙). 정확한 화소 규칙은 M4 채우기 구현 때 fixture로 고정한다.

M4-3에서 고정한 규칙(2.2.13 `applyFillStroke`와 같음): coverage ∩ clip 안의 화소는 채우기 색(premultiplied)으로 **대체**한다(위에 덮어 그리지 않는다). `antialias`면 coverage 밖이면서 4-이웃 중 하나가 coverage 안인 화소 ∩ clip에 `기존 + 색 × (1 − 기존 alpha)`로 뒤에 깐다. 브러시 불투명도는 쓰지 않고 색의 alpha만 쓴다. 시험: `semantics::a_fill_replaces_what_it_covers_and_goes_behind_its_edge`(참조 평가기), `document::tests::a_fill_replaces_what_it_covers_and_goes_behind_its_edge`(렌더러, 덮은 화소는 바이트 단위로 같고 가장자리는 1단계 이내).

## 3. 결정: 병합은 isolated section으로 표현한다

아래 레이어 B 위로 위 레이어 A를 병합하면 결과 레이어는 다음과 같다.

- B가 불투명(opacity 1)이면 `B.ops + [Isolated(A.ops, A.opacity, A.wobble)]`
- 아니면 `[Isolated(B.ops, B.opacity, B.wobble), Isolated(A.ops, A.opacity, A.wobble)]`
- 결과 레이어는 Normal, 불투명도 1, wobble은 B의 것

그래서 다음이 성립한다(시험 `merging_keeps_every_frame_and_each_eraser_in_its_own_layer`, `an_eraser_added_after_a_merge_reaches_both_layers`).

- 모든 프레임의 모양이 병합 전과 같다.
- 병합 전 A의 지우개는 A 구간만 지운다.
- 병합 뒤에 추가한 지우개는 두 레이어의 합성 결과에 작용한다.

2.2.13은 두 레이어가 모두 불투명도 1이고 모션 설정이 같을 때만 병합을 허용했다. section이 불투명도와 wobble을 가지므로 3.0에서는 이 두 조건을 없앤다. section의 wobble이 `None`이면 레이어와 마찬가지로 문서의 wobble을 따른다.

계속 거절하는 경우(`MergeRefusal`, 시험 `merging_refuses_what_it_cannot_keep`):

| 거절 | 이유 |
|---|---|
| `Blend` | Normal이 아닌 합성 모드는 아래 레이어들과의 관계로 정의되므로, 레이어 안 구간으로 옮기면 모양이 바뀐다 |
| `Clipping` | 클리핑 관계가 바뀐다 |
| `CanvasEpoch` | A가 시작한 캔버스가 B가 끝난 캔버스와 다르다 |
| `CanvasChange` | A 안의 자르기·리사이즈가 B까지 움직인다. 불투명하지 않은 B의 자르기·리사이즈도 section 안에 들어가므로 거절한다 |

병합을 거듭하면 section이 중첩된다. 깊이는 문서 검증에서 16단계로 제한한다(`ugu_core::document::limits::SECTION_DEPTH`). 참조가 아니라 값으로 담으므로 순환은 생길 수 없다.

## 4. 결정: `.ugurugu` 파일 스키마 1

계획 8.1절의 ZIP 컨테이너를 따른다. 확장자는 `.ugurugu`다. 2026-10-07에 `.ugu2`로 확정했다가, 2026-10-08 사용자 결정으로 바꿨다. "2"가 앱 2.x로 읽히고, 형식이 바뀌어도 확장자는 그대로 두고 schema로 구분하기 때문이다. 2.x의 `.ugu`와는 겹치지 않는다.

```text
manifest.json        {"format":"ugurugu-document","schema":1,"render_revision":1,
                      "document_id":"<uuid>","required":[]}
document.json        캔버스·배경·프레임 수·FPS·문서 wobble, 레이어 트리, 연산, 획·마스크·자산 표
strokes.bin          모든 획의 점 데이터
masks.bin            모든 마스크의 비트
images/<sha256>.png  래스터 자산 (RGBA8, straight alpha, 정규화한 PNG)
```

처음 안에는 선택 항목 `thumbnail.png`(파생 데이터)도 있었으나 구현하지 않았다. writer는 쓰지 않고, reader는 위 목록 밖의 항목을 손상으로 거절하므로 `thumbnail.png`가 든 파일도 열지 않는다(`crates/ugu-io/src/read.rs` `known_name`).

- **document.json**: 레이어는 아래부터 순서대로 배열하고, 그룹은 자식 레이어를 `group.children`에 같은 방식으로 담는다(처음 안의 `parent` id가 아니라 중첩). 연산은 `{"op":"paint","stroke":7,"clip":null}`처럼 태그를 붙인 객체이고, `isolated`는 `ops`를 중첩한다. `PlaceImage`의 태그는 `image`다. 획 표 항목은 `id, color("#rrggbbaa"), width, brush{...}, seed`다. seed는 JSON 숫자의 정밀도(2^53)를 넘으므로 16진 문자열로 둔다. 좌표는 문서 화소이며 viewport 상태는 저장하지 않는다.
- **strokes.bin**: `"UGS\0"`, u16 version=1, u16 flags=0, u32 획 수, 이어서 id 순으로 획마다 u32 id, u32 점 수, 점마다 f32 x, f32 y, f32 pressure. 모두 little-endian이다. Rust 구조체를 덤프하지 않고 이 순서로 직접 쓰고 읽는다. f32는 4096² 캔버스에서 약 0.0005px 정밀도라 충분하다.
- **masks.bin**: `"UGM\0"`, u16 version=1, u16 flags=0, u32 마스크 수, 이어서 id 순으로 마스크마다 u32 id, i32 left, top, width, height, 행마다 MSB가 왼쪽인 비트열(행 단위 byte padding, 남는 비트는 0). 2.2.13 `PackedMaskRegion`과 같은 배치다.
- **한 항목에 모으는 이유**: 처음에는 획마다 `strokes/<id>.bin` 항목을 두었다. 그러자 짧은 획 20,000개(fixture ④)의 저장이 646ms로 2.2.13(286ms)보다 느렸다. 원인은 데이터 양이 아니라 ZIP 항목마다 드는 고정 비용(로컬 헤더, Deflate 스트림 시작)이었다. 메모리로 인코딩만 해도 313ms가 걸렸다. 한 항목으로 모은 뒤 저장은 57ms, 열기는 226ms에서 25ms가 됐다(evidence.md 24절). 읽을 때는 레코드가 id 순으로 엄격히 증가하고 항목을 정확히 덮어야 하며, `document.json`의 획·마스크 표와 일대일로 맞아야 한다.
- **images**: 이름이 내용의 SHA-256이므로 같은 이미지를 여러 번 배치해도 한 번만 저장한다. PNG는 다시 압축하지 않고 ZIP에 stored로 넣는다. 나머지 항목은 Deflate(`zip` crate의 pure-Rust `zlib-rs` backend)로 넣는다.
- **읽기 검증**: 항목 수·이름 중복·경로(디렉터리 탈출 금지)·압축/해제 크기·실제 해제량 누계·이미지 화소 수·좌표 유한성·연산 수·section 깊이·id 참조를 제한하고 검사한다. manifest 선언값만 믿지 않는다. `schema`·`render_revision`이 모르는 값이거나 `required`에 모르는 기능이 있으면 거절한다. 구형 `.ugu`는 schema 13만 가져오기로 변환하고(2026-10-10, decisions.md 파일·형식·가져오기), 그 밖의 구형 파일은 "지원하지 않는 형식" 판별만 한다(scope.md 4절).
- **저장**: 같은 디렉터리의 임시 파일에 쓰고 검증·flush한 뒤 교체한다(계획 8.2절). 작업자 스레드에서 직렬화한다.

## 5. 결과

- 연산 enum에 mode와 무관한 optional 조합이 없다. `TransformSelection`에 `drawDestination=false`가 섞이는 대신 `ClearSelection`이 따로 있다.
- 병합이 2.2.13보다 넓게 허용된다(불투명도·모션 차이).
- 채우기는 움직이지 않는다. 2.2.13의 새 채우기와 같은 모습이다.
- 이 ADR 때 남겼던 undo delta 형식(M1), 브러시·모션 필드(M2·M4), 문자 도구의 연산(M4. 적용 시 윤곽을 `Fill`과 `Paint`로 확정)은 각 단계에서 정했다. 파일에 더한 필드는 7절에 모았다.

## 6. 사용자 확인 (2026-10-07)

1. 파일 확장자는 `.ugurugu`로 한다.
2. 병합 조건 완화(불투명도·모션 차이가 있어도 병합 허용)를 3.0 동작으로 받아들인다.

## 7. ADR 뒤에 schema 1에 더한 것 (2026-10-12 정리)

3.0은 출시 전이라 schema 번호를 올리지 않고 schema 1을 제자리에서 넓혔다(decisions.md 파일·형식·가져오기). 그래서 이 사이의 개발 빌드가 저장한 파일은 열지 못할 수 있다. 3.0.0 출시부터는 schema 1을 고정하고, 바꾸려면 schema를 올리거나 `required`에 기능 이름을 넣는다(4절 읽기 검증). 필드 이름은 `crates/ugu-io/src/format.rs`에서 확인했다. 모든 JSON 구조체는 `deny_unknown_fields`라 모르는 필드가 있으면 열지 않는다.

`document.json`:

| 위치 | 필드 | 더한 때 | 내용 |
|---|---|---|---|
| 획 `brush` | `size_dynamics`, `wobble_scale` | M2 | 굵기 필압 0~1, 흔들림 배율 0~2 |
| 획 `brush` | `engine`(`line`·`airbrush`·`spray`·`pixel`), `tip`(`round`·`square`), `opacity`, `flow`, `hardness`, `spacing`, `scatter`, `particle_size`, `density`, `opacity_dynamics`, `size_jitter`, `animated_jitter`, `antialias` | M4-6 | 2.2.13 `BrushSettings`의 모든 필드. `pixel`은 M5-16 픽셀 연필 |
| 문서·레이어·`isolated`의 `wobble` | `amount`, `style`(`classic`·`smooth`·`stepped`), `poses`, `detail`, `linked`, `randomness`, `broken`, `break_amount`, `break_range` | M2(`style`), M4-7(나머지) | 2.2.13 `MotionSettings`. `poses`는 1~60을 그대로 두고 렌더할 때 프레임 수로 줄인다 |
| 문서 | `pixel` | M6-17 | 문서 단위 픽셀 모드(decisions.md 픽셀 모드). 꺼져 있으면 쓰지 않으므로 그 파일의 바이트는 전과 같다. render revision은 그대로 |
| 문서 | `active_layer` | M6(2절 23) | 저장할 때 편집하던 페인트 레이어 id. 선택 필드이고, 열 때 페인트 레이어가 아니면 무시한다. 프레임은 저장하지 않는다(2.2.13과 같음). API는 `ugu_io::write_with`·`read_with`·`save_with` |

그 밖의 검증 한도(`ugu_core::document::limits`): 레이어 256개, 그룹 깊이 9단계(2.2.13 파일을 받으려고 8에서 올림, ugu-import.md 2절 3), 연산 20,000개, section 깊이 16단계, 캔버스 한 변 1~4096, 프레임 2~240(`limits::FRAMES`, 3.3.0부터이고 3.2 이하는 60). 저장소(`ugu_core::store`): mask는 0~4096 안, 이미지 화소 합 4096²×64. 읽기(`ugu_io::read::limits`): 획 20,000개, mask 40,000개, 이미지 20,000개, 파일 길이 약 223MiB.

`.ugurugu` 밖의 형식(앱, `apps/ugurugu/src`):

| 파일 | 머리 | 내용 |
|---|---|---|
| 복구 설명 `recovery\<창 폴더>\tab-<n>\<0·1>.json` (M5-6, 탭마다는 문서 탭) | `format: "ugurugu.recovery"`, `version: 1` | `sequence`, `name`, `path`, `time`, `canvas`, `layer`(M6에 더함, 활성 레이어). 문서는 같은 칸의 `<0·1>.ugurugu`이고 두 세대를 번갈아 쓴다. 창 폴더는 실행하는 동안 `lock` 파일로 잠근다. 탭 전의 3.0이 창 폴더에 바로 쓴 `<0·1>`도 읽는다. 폴더는 `%LOCALAPPDATA%\Ugurugu Data\3\recovery`(`recovery.rs`) |
| 설정 `%APPDATA%\Ugurugu\3\settings.json` (M5-3) | `format: "ugurugu.settings"`, `version: 1` | `language`, `accent`, `theme`(`dark`·`light`·`system`, 기본인 `dark`이면 쓰지 않음), `wobbleAnimation`, `pixelGrid`, `defaultSaveFolder`, `exportSize`, `pixelExportScale`(픽셀 모드 내보내기의 정수 배, 1·2·4·8), `checkForUpdates`(끈 때만 씀), `tools`(도구 상태, 필압 켬·끔은 `tools.tablet.pressure`), `colorHistory`, `shortcuts`(기본과 다른 키만), `panels`(도크 배치), `window`(`left`·`top`·`right`·`bottom`·`maximized`, 닫을 때의 창, 작업 영역 픽셀). 기본값인 선택 키는 쓰지 않는다. 모르는 키는 지우지 않고 다시 쓴다. 여러 창은 `settings.json.lock`으로 차례를 지키고 바꾼 최상위 키만 덮어쓴다(decisions.md UI·단축키·번역). `settings.rs` |
| 도구 프리셋 `*.ugurugu-tools` (M5-10) | `format: "ugurugu.tools"`, `version: 1` | `tools`(설정의 `tools`와 같은 글, `tablet` 제외)와 `motion`(`amount`, `style`, `poses`, `detail`, `linked`, `randomness`, `broken`, `breakAmount`, `breakRange`). 1MiB 상한, 모르는 키는 무시하고 있는 값만 덮어쓴다. 2.2.13 `.wwpreset`은 읽지 않는다. `settings/preset.rs` |

## 8. 키프레임·애니메이션 폴더와 기능 이름 (3.2.0, 3.3.0)

3.2.0은 schema 1에 선택 필드 둘을 더한다. 쓰지 않으면 생략해 3.1까지의 문서는 바이트가 그대로이고, 쓴 문서는 manifest `required`에 기능 이름을 넣어 3.1 이하가 "새 판에서 만든 파일"로 거절하게 한다(`deny_unknown_fields`만으로는 "손상된 파일"이 된다). 읽기는 `required`에 아는 이름(`ugu_io::format::FEATURES`)만 있을 때 연다. `render_revision`은 1 그대로다(키·셀 없는 문서의 화소는 바뀌지 않음). 필드 이름은 `crates/ugu-io/src/format.rs`, 의미는 `crates/ugu-core/src/keys.rs`와 decisions.md 4절 "키프레임과 애니메이션 폴더"다.

| 위치 | 필드 | `required` | 내용 |
|---|---|---|---|
| `paint`·`group` | `keys` | `"keys"` | `pivot`(문서 px [x, y]), `keys`(프레임 순): `frame`(0 ≤ frame < 문서 프레임 수, 프레임마다 하나), `offset`(px, ±32767), `rotation`(도, 시계 방향, ±3600), `scale`(0.05~20), `flip`(켜졌을 때만 씀), `opacity`(0~1, 레이어 불투명도에 곱함, 1이면 쓰지 않음, 3.3.0), `ease`(`linear`·`smooth`·`hold`, 이 키에서 다음 키까지) |
| `group` | `cels` | `"cels"` | 애니메이션 폴더의 노출표(프레임 순): `frame`, `cel`(그 그룹의 바로 아래 자식 id, `null`은 빈 프레임). 다음 노출 전까지 유지, 첫 노출 전 프레임은 마지막 노출 |

3.3.0은 같은 방식으로 기능 이름 셋을 더했다(`ugu_io::write`의 `required`가 쓰는 문서를 보고 넣는다). 지금 `FEATURES`는 `keys`·`cels`·`frames`·`key-opacity`·`fill-behind` 다섯이다.

| `required` | 넣는 때 | 3.2 이하가 모르고 읽으면(`format.rs` 주석) |
|---|---|---|
| `"frames"` | 프레임 수가 60을 넘을 때(`limits::FRAMES_BEFORE_3_3`) | 3.2 이하가 읽는 것보다 프레임이 많다 |
| `"key-opacity"` | 어떤 키의 `opacity`가 1이 아닐 때 | 레이어가 서서히 사라지는 것을 빼고 읽는다 |
| `"fill-behind"` | `fill` 연산 하나라도 `behind: true`일 때(그룹·병합 section 안 포함) | 채우기를 선 뒤가 아니라 위에 덮어 그린다 |

키가 있는 레이어의 화소는 그 프레임의 자세를 중심에 대해 크기·반전 → 회전 → 이동 순으로 적용한 위치에 그린다. 그룹의 키는 안의 레이어마다 자기 키 뒤에 곱한다(그룹 결과를 재샘플하지 않음). 검증 위반은 `DocumentError::Animation`이다.
