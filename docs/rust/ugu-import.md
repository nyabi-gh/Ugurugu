# 2.x `.ugu` schema 13 가져오기 대응표 (M5-18, 2026-10-11)

m5-plan 2절 15의 첫 단계다. 2.2.13 writer는 `src/io/serializer/DocumentJsonCodec.cpp`이고, reader는 `DocumentSerializer.cpp`와 `DocumentValidation.cpp`다. 3.0 쪽은 `crates/ugu-core`(document.rs, ops.rs, store.rs)다.

## 1. 대응

| 2.2.13 (schema 13) | 3.0 | 변환 |
|---|---|---|
| `canvas{width,height,background "#AARRGGBB"}` | `Document.canvas`, `background` | 배경은 ARGB를 straight `Rgba8([r,g,b,a])`로 바꾼다 |
| `animation{frames,fps,wobble,motion}` | `frames`, `frames_per_second`, 문서 `Wobble{amount, Motion}` | `poseCount`→`poses`, `brokenLine`→`broken`. 값 범위는 2.2.13 검사(`isValidMotionSettings`)와 같게 다시 검사 |
| `layers[]` 평면 배열 + `parentGroupId` | `Layer` 트리(paint/group) | 배열 순서를 지키고 id는 `LayerId(u32)`로 다시 매김. opacity·blend·visible·reference·name·clipToLayerBelow·`initialCanvasSize`는 1:1. 레이어 `wobble`+`motion` → `PaintLayer.wobble` |
| `activeLayerId` | 세션의 현재 레이어 | 문서에는 넣지 않음 |
| paint / erase 획 | `Store.strokes` + `Op::Paint` / `Op::Erase` | `points[[x,y,p]]`(p 기본 1), color, width, seed(10진 u64), brush 필드 1:1(범위는 `brush_in_range`가 2.2.13과 같음) |
| `clipMaskId`(Gray8) ∩ `visibilityClip` | 1bit `Mask` | 128 이상을 켬으로(2.2.13 `materializedVisibilityMask`와 같은 계산). (마스크, 사각형) 쌍마다 하나 |
| fill + `fillCoverageId` | `Op::Fill{coverage, color, antialias, clip}` | 비트 배치가 같아 그대로 복사. 칠 규칙은 ADR 2절에서 이미 같게 옮김 |
| fill + `fillMaskId`(schema 5 얼린 채우기, Gray8) | `Op::Fill` | 128 이상을 비트로, antialias는 켬으로 고정(2.2.13은 이 경우 가장자리를 건너뛰지 않음) |
| image `{assetId, transform, sampling}` + `rasterAssets` | `Op::PlaceImage` + PNG 자산 | qCompress straight RGBA를 풀어 `import::from_rgba`로 PNG, id 다시 매김 |
| `pixelSelection{clearSource, drawDestination}` | `TransformSelection{keep_source}` / `ClearSelection` | (참,참)→keep_source=false, (거짓,참)→true, (참,거짓)→ClearSelection |
| QTransform 9개 | `Affine` 6개 | `[m11,m21,m31,m12,m22,m32]`. m13·m23≈0, m33≈1이 아니면 거절(2.2.13도 무효) |
| `reframe` canvas 모드 | `Crop{offset: contentOffset, size}` | 부호 같음 |
| `reframe` image 모드 | `Resample{size, sampling}` | |
| `compositeBoundary`(병합 경계) | `Op::Isolated(Section{ops, opacity 1, wobble: 레이어의 것})` | 첫 경계 앞은 그대로, 경계 사이 구간마다 section, 마지막 경계 뒤는 그대로. section의 wobble을 비우면 문서 wobble을 따르므로 레이어 값을 넣는다 |
| 문자 | 그대로(Fill + Paint 획) | 2.2.13도 확정 때 획으로 구움 |
| 버리는 것 | | 획·레이어 UUID, 그룹의 `initialCanvasSize`, fill·image·selection 획의 seed·brush, 마스크·자산의 원래 id(그림에 영향 없음) |

## 2. 옮길 수 없거나 확인이 필요한 경우와 처리 (판단 위임에 따라 정함)

| # | 경우 | 처리 | 이유 |
|---|---|---|---|
| 1 | procedural fill(coverage도 fillMask도 없는 fill, schema 4 이전에서 다시 저장된 파일) | **열지 않고 이유를 보임** | 매 프레임 그때의 레이어를 flood fill한다. 3.0에 이 표현이 없고(ADR "옮기지 않는다"), 한 프레임에서 얼리면 선이 움직일 때 모양이 달라져 조용한 변경이 된다 |
| 2 | 첫 병합 경계와 마지막 경계 사이에 있는 reframe | **열지 않고 이유를 보임** | 2.2.13은 끝난 구간에도 reframe을 적용하고, 3.0은 section 안 Crop·Resample을 막는다. section을 쪼개는 근사는 Resample에서 합성 순서가 바뀌어 화소가 달라진다 |
| 3 | 그룹 9단계 | **3.0 상한을 8 → 9단계로 올림** | 2.2.13은 0~8(9단계)까지 편집을 허용한다. 상한 하나 차이로 실제 파일을 거절하지 않는다 |
| 4 | 한 변이 16384를 넘는 래스터 자산 | 열지 않고 이유를 보임 | 2.2.13은 화소 수만 제한해 가능하지만 3.0 자산 상한을 넘는다. 줄여 넣으면 변형을 보정해야 하고 화소가 달라진다 |
| 5 | 3.0 저장 예산(128MiB, PNG 포함)을 넘는 문서 | 열지 않고 이유를 보임 | 경계에 걸린 문서만 해당한다 |
| 6 | `#AARRGGBB`·`#RRGGBB`가 아닌 색 문자열 | 열지 않고 이유를 보임 | writer는 항상 `#AARRGGBB`를 쓴다. 다른 형식은 손으로 고친 파일이다 |
| 7 | 좌표·굵기·브러시 값의 f64 → f32 | 받아들임 | 4096에서 약 0.0002px. fixture 렌더 비교의 허용치(M4 회귀와 같음)로 확인 |
| 8 | smooth 샘플링, clipToLayerBelow 규칙, Overlay 공식이 같은지 | fixture로 확인 | 이 조사에서는 코드만 보고 같다고 단정하지 않았다 |

## 3. fixture

- `examples/kimcozo_service.ugu`는 실제 schema 13 파일이다(레이어 7, paint 185, erase 14, fill 52, 선택 이동 2).
- `tools/FixtureGenerator.cpp`는 paint·airbrush·spray·erase·그룹·blend 4종·clipping·image·선택 이동만 쓴다.
- 빠진 것: fill, 자르기·리사이즈, 병합 경계, 레이어 모션, smooth·stepped·broken, reference, clip 마스크, ClearSelection·keep_source, legacy fillMask, procedural fill.
- 계획: `tests/DocumentSchemaTests.cpp`의 생성 코드를 FixtureGenerator로 옮겨 전 기능 fixture를 더하고, 2.2.13과 3.0으로 렌더해 모든 프레임을 비교한다. procedural fill 예는 `tests/fixtures/legacy-render/*.wagle`을 2.2.13에서 다시 저장해 얻는다(거절 시험용).
- 연결 지점: `crates/ugu-io/src/read.rs`의 `legacy()`가 이미 JSON을 `Legacy::Json`으로 판별한다.
