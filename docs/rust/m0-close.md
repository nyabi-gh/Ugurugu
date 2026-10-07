# M0 마무리: 기술 조합, 버전 고정, M1 분해 (15일차, 2026-10-07)

근거는 [m0-evidence.md](m0-evidence.md), 범위와 결정은 [scope.md](scope.md), 연산·파일 ADR은 [adr-operations-and-format.md](adr-operations-and-format.md)에 있다.

## 1. 기술 조합 (확정)

| 영역 | 채택 | 근거 (m0-evidence) | 기각한 것 |
|---|---|---|---|
| 창·이벤트 | winit 0.30.13 + `WM_POINTER` subclass(`ugu-win`) | 3절: coalesced 마우스 이동 복원, 포인터 history | — |
| UI | egui 0.36.2 (egui-winit, egui-wgpu) | 7절: 한국어·일본어 IME, 100~200% 배율, UI Automation 노출 | iced 비교는 IME가 통과해 불필요 |
| 화면 표시 | wgpu 30.0.1, DX12만 | 1·4·8절: 창 전체 swapchain 하나, Independent Flip에서 입력→표시 p50 약 6ms(C++보다 약 4ms 빠름), 장치 손실 복구, WARP fallback | 자식 창 swapchain(4절), Vulkan backend(계획 4.3절) |
| 문서 렌더 | Vello CPU 0.3.0, SSE4.2 고정 | 6절: 1스레드 래스터화가 tiny-skia의 3.4배, 8스레드 약 24배 | tiny-skia, Vello GPU(래스터화가 8스레드 CPU보다 5배 느림), skia-safe, Direct2D |
| 접근성 | AccessKit(egui-winit 기능), 어댑터는 UI 스레드 | 7절: 이름 있는 컨트롤, 포커스·입력·Invoke | Narrator 청취 검증(범위 제외) |
| 파일 | `.ugu2` ZIP 컨테이너, schema 1 | ADR 4절 | 구형 `.ugu` reader |
| WebP | 내보내기 인코딩만 C `libwebp` 1.6.0, 가져오기는 `image-webp` | scope.md 3절 | 순수 Rust WebP 인코더 |

M0 앱(`apps/ugurugu`)의 probe UI(IME 칸, 지연 표시, 진단 키)는 실증용이다. M2에서 제품 UI로 바꾸며, 진단 기능은 `UGURUGU_DIAGNOSTICS=1`일 때만 남긴다.

## 2. 버전 고정

- **툴체인:** `rust-toolchain.toml`의 `1.99.0`.
- **직접 의존:** workspace의 모든 외부 crate를 `=` 버전으로 고정한다. 기능은 필요한 것만 켠다(`default-features = false`).
- **잠금 파일:** `Cargo.lock`을 커밋하고, CI는 `--locked`로 빌드·시험한다. `cargo deny`가 보안 권고·라이선스·출처를 검사한다.
- **올리는 절차:** 버전 올리기는 따로 PR로 한다. 다음 시험이 이전과 같거나 나아야 받아들인다.
  - `latency-probe`(입력→표시)
  - `ime_probe.py ko/ja`
  - `uia_probe.ps1`
  - 장치 손실 시험(`UGURUGU_DIAGNOSTICS=1`, F9)
  - `render-bench`(Vello 결과가 바이트 단위로 같은지)
- **M1에서 넣을 crate:**

| crate | 버전 | 기능 | 비고 |
|---|---|---|---|
| zip | =8.6.0 | `deflate-flate2-zlib-rs`만 | 기본 기능은 zstd·bzip2 같은 C 라이브러리를 끌어오므로 끈다. 9.x는 아직 pre-release |
| serde / serde_json | =1.0.229 / =1.0.151 | `derive` | `document.json`, `manifest.json` |
| sha2 | =0.11.0 | — | 자산 이름(SHA-256) |
| uuid | =1.27.0 | `v4` | 문서 id |
| proptest | =1.11.0 | dev 전용 | 잘못된 입력 거부, roundtrip 시험 |

## 3. M0 종료 조건 대비

계획 11절의 M0 종료 조건은 "실제 Windows 펜·IME·DPI 입력 성공, 렌더 후보·의존성 결정, 2.2.13 측정값 기록"이다.

| 조건 | 상태 |
|---|---|
| 펜 입력 | **미충족, 이월.** 펜 장치가 없다. 마우스로 `WM_POINTER` 경로만 확인했다. 장치 없이 통과시키지 않고 M6 이전 필수 gate로 둔다(scope.md 5절). WinTab 필요성 결정도 같이 미룬다 |
| IME | 충족. 한국어·일본어, 후보창 위치, Enter 누출 없음 |
| DPI | 충족. 100/125/150/200% |
| 렌더 후보·의존성 | 충족. 1·2절 |
| 2.2.13 측정값 | 충족. 입력→표시, 프레임 렌더, UI 스레드 CPU·점유, pen-up, 저장·열기, 취소, RAM, 재생 fps, 내보내기 중 UI 반응 |

펜을 뺀 나머지가 충족됐으므로 M1을 시작한다. 펜 결과가 나쁘게 나오면 바꿀 수 있는 곳은 입력 경로(`ugu-win`)로 한정된다. M1(도메인·저장)은 펜과 독립적이다.

## 4. 위험표 (M0 결과 반영)

| 위험 | 영향 | 현재 상태 | 해소 시점 |
|---|---|---|---|
| 펜 경로 미검증 | 필기 품질, pen-up 손실 | 장치 없음 | 장치 확보 후, 늦어도 M6 전 |
| egui-wgpu가 장치 손실 때 panic | 렌더 스레드 종료 | 장치가 손실됐을 때만 `catch_unwind`로 흡수 | 상류에서 오류를 돌려주게 고쳐지면 제거 |
| 창 모드에서 DWM 승격이 바탕화면 상태에 따름 | 입력→표시가 6ms에서 11~12ms로 | 최대화 창은 항상 승격 | M3에서 원인 조사 |
| WARP 지연(60~75ms) | GPU 없는 PC에서 느림 | 안내와 함께 fallback(사용자 결정) | 수용 |
| 2.2.13 pen-up이 18~21ms(목표 8ms의 2~3배) | 3.0도 같은 구조면 같은 문제 | 기준선으로 기록 | M1 history·M2 commit 설계에서 목표로 잼 |
| 취소가 프레임 경계에서만 확인(2.2.13 p95 최대 3.7s) | 큰 문서에서 늦은 취소 | 계획 목표 p95 100ms | M3 타일 단위 취소 |
| 새 형식 저장·열기가 2.2.13보다 느려짐 | 회귀 | 2.2.13: 저장 20~286ms, 열기 118~561ms | M1-10에서 측정 |
| egui TextEdit의 SetValue 미지원 | 일부 보조 기술 제한 | 기록 | 수용 |
| 실제 TDR, 원격 세션 미시험 | 드문 환경에서 미확인 | RemoveDevice·WARP로 대체 확인 | TDR은 M3, RDP는 M6 |

## 5. M1 작업 분해 (도메인·새 저장, 계획 2~3주)

M1 종료 조건은 "roundtrip·거부 입력·macro 원자성·save failure 검증"이다. 각 행은 시험을 포함한 하나의 변경(PR 단위)이고, 순서대로 의존한다.

| # | 변경 | crate | 완료 기준(시험) |
|---|---|---|---|
| M1-1 | 문서 타입: `Document`, 레이어 트리(Paint/Group), `LayerId`·`StrokeId`·`MaskId`·`AssetId`, 캔버스·프레임·FPS·wobble | ugu-core | 부모·순환·깊이·레이어 수 제한을 검증이 거절한다 |
| M1-2 | 데이터 저장소: 획 점(f32 x·y·pressure)·브러시·seed, 1비트 마스크, 자산 바이트를 `Arc`로 공유하고 바이트를 집계 | ugu-core | 복제해도 큰 배열을 복사하지 않는다. 바이트 합계가 맞는다 |
| M1-3 | 명령과 커밋: 연산 추가, 레이어 추가·삭제·이동·속성, `merge_down`. 검증 후 한 번 커밋하고 `Committed`/`NoChange`/오류를 구분. 단조 증가 `Revision` | ugu-core | 변경 없음은 undo 항목을 만들지 않는다. 잘못된 명령은 문서를 바꾸지 않는다 |
| M1-4 | undo/redo와 macro: delta history, macro 중간 실패 시 전체 되돌림, 저장 지점 identity로 dirty 판정 | ugu-core | 저장 지점으로 undo하면 dirty가 아니다. macro 실패 전후 문서가 같다. 긴 history의 커밋 비용을 잰다 |
| M1-5 | `.ugu2` 쓰기: manifest, `document.json`, `strokes.bin`, `masks.bin`, `images/<sha256>.png` | ugu-io | 같은 문서는 같은 바이트로 저장된다. wire format의 바이트 배치 시험 |
| M1-6 | `.ugu2` 읽기와 거부: 항목 수, 이름 중복, 경로, 압축·해제 크기, 실제 해제량 누계, 좌표 유한성, 연산 수, section 깊이, id 참조, 모르는 schema·required | ugu-io | roundtrip이 같다. 잘라낸 파일, 부풀린 크기, 순환 참조, NaN 좌표 등 거부 시험(proptest 포함) |
| M1-7 | 안전한 저장: 같은 폴더 임시 파일, 검증, flush, 교체(`ReplaceFileW`, 처음이면 이동). 실패하면 기존 파일과 dirty 상태를 유지 | ugu-io, ugu-win | 실패 주입(디스크 가득, 권한 없음, 공유 위반) 뒤 기존 파일이 그대로다 |
| M1-8 | 구형 파일 판별: `.ugu`·`.wagle`·`.wobble`·`.wawa`·`.wwpreset`을 알아보고 "지원하지 않는 형식"으로 거절 | ugu-io | 구형 fixture 각각이 같은 오류를 낸다 |
| M1-9 | headless 도구: fixture ①~⑤를 새 형식으로 만드는 생성기, 문서 검증·요약 출력 | tools | 생성 결과가 manifest(레이어·획·점 수)와 맞는다 |
| M1-10 | 측정: 새 형식의 파일 크기, 저장·열기 시간을 같은 PC에서 2.2.13 값과 비교 | tools | m0-evidence 5절 표에 나란히 기록한다. 나쁘면 원인과 판단을 남긴다 |

M1에서 하지 않는 것: 화면 UI 연결(M2), 렌더러와 문서의 연결(M2), 자동복구(M5).

**진행 (2026-10-07):** M1-1~M1-10 모두 완료(`rust/m1`). 결과는 m0-evidence 9절.

## 6. 사용자 확인이 필요한 것

1. 브랜치: M1을 `rust/m0`에서 이어갈지, `rust/m0`를 정리해 main에 합친 뒤 새 브랜치에서 할지. 계획 12절은 C++ 트리를 정식 완료 때 제거한다고 했으므로, 그 전까지 main에는 두 트리가 함께 있게 된다.
