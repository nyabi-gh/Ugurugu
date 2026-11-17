# 문서 안내

## 지금 상태

- Ugurugu 3(Rust, `apps/`, `crates/`)은 3.0.0(2026-10-13)부터 Windows판을 냈고, 3.6.3(2026-11-17)부터 macOS(Apple Silicon)·Linux(x64)판도 함께 낸다. 판마다 바뀐 것은 `release-notes/`와 GitHub 릴리스에 있다. 열린 일은 [backlog.md](rust/backlog.md)에 있고, macOS·Linux의 남은 일은 그 3절에 있다.
- 2.2.13(C++)과 웹판은 더 고치거나 내지 않는다. 그 소스는 2026-10-13에 저장소에서 지웠고 `v2.2.13` 태그에 있다. 문서에 나오는 `src/`, `tools/*.cpp`, `tests/*.cpp` 같은 C++ 경로는 그 태그 기준이다.

## 3.0 문서 (`rust/`)

| 문서 | 내용 |
|---|---|
| [scope.md](rust/scope.md) | 제품 범위: 기능, 플랫폼, 하드웨어, 뺀 것, 측정 PC |
| [decisions.md](rust/decisions.md) | 설계 결정과 2.2.13과 다르게 정한 점. 출처 칸의 옛 ID(M5-17 등)로 코드 주석에서 찾는다 |
| [evidence.md](rust/evidence.md) | 지금 코드가 기대는 측정과 시험 결과, 측정 방법 |
| [backlog.md](rust/backlog.md) | 열린 일을 종류별로: 사용자 할 일, 열린 gate, macOS·Linux 실제 장치 확인, 미룬 결정, 알려진 한계, 개선 후보, 측정·확인 공백 |
| [adr-operations-and-format.md](rust/adr-operations-and-format.md) | 편집 연산의 의미와 `.ugurugu` 형식 |
| [ugu-import.md](rust/ugu-import.md) | 2.x `.ugu` schema 13 가져오기 대응표 |

새 결정은 decisions.md에, 새 측정은 evidence.md에, 열린 일은 backlog.md에 적는다. 같은 내용을 여러 문서에 다시 쓰지 않는다. 일을 닫으면 backlog.md의 행과 다른 문서의 그 ID도 함께 지운다. CI의 `tools/check_docs.py`가 문서 링크와 backlog ID를 검사한다.

## 지운 문서

계획 문서는 일이 끝나면 지운다. 결정은 decisions.md, 측정은 evidence.md, 남은 일은 backlog.md로 옮겼다. 문서에 남은 "m6-plan 2절", "plan-cross-platform 3.3절" 같은 출처는 아래 커밋에서 읽는다(`git show 70f9454:docs/rust/m6-plan.md`).

| 지운 날 | 문서 | 원문 |
|---|---|---|
| 2026-10-12 | 마일스톤 계획(`RUST_WINDOWS_PORT_PLAN.md`, `rust/m0-close.md`, `rust/m2-plan.md`~`m6-plan.md`), 옛 측정 기록(`rust/m0-evidence.md`), 2.x 상태 문서(`project-status.md`), 검토 보고서(`ANALYSIS_REPORT.md`, `analysis-3.0.md`, `RUST_3_REVIEW_2026-10-12.md`)와 그 원자료(`review-evidence-*`) | `70f9454` |
| 2026-10-13 | 문서 탭·UI 다듬기 계획(`rust/plan-document-tabs.md`) | `70c27f4` |
| 2026-10-14 | 3.2.0 애니메이션 계획(`rust/plan-animation.md`) | `d0506ba` |
| 2026-11-17 | macOS·Linux 지원 계획(`rust/plan-cross-platform.md`) | `ced89bd` |

## 그 밖

사용자 안내는 루트 README(ko·en·ja), 빌드는 BUILDING.md, 기여는 CONTRIBUTING.md, 보안은 SECURITY.md, 판별 변경 내용은 `release-notes/`에 있다. 제3자 고지는 저장소에 두지 않고 패키지를 만들 때 `tools/third_party_notices.py`가 `THIRD_PARTY_NOTICES.txt`로 만든다.
