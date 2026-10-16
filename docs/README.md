# 문서 안내

## 지금 상태

- Ugurugu 3(Rust, `apps/`, `crates/`)은 Windows판으로 3.0.0, 3.0.1, 3.0.2(2026-10-13), 3.1.0과 3.2.0(2026-10-14), 3.2.1과 3.3.0(2026-10-15), 3.3.1과 3.3.2(2026-10-16)를 냈다. 열린 일은 [backlog.md](rust/backlog.md)에 있다.
- 2.2.13(C++)과 웹판은 더 고치거나 내지 않는다. macOS는 2.2.13이 마지막이고, macOS 3.x는 나중에 따로 한다. 그 소스는 2026-10-13에 저장소에서 지웠고 `v2.2.13` 태그에 있다. 문서에 나오는 `src/`, `tools/*.cpp`, `tests/*.cpp` 같은 C++ 경로는 그 태그 기준이다.

## 3.0 문서 (`rust/`)

| 문서 | 내용 |
|---|---|
| [scope.md](rust/scope.md) | 제품 범위: 기능, 플랫폼, 하드웨어, 뺀 것, 측정 PC |
| [decisions.md](rust/decisions.md) | 설계 결정과 2.2.13과 다르게 정한 점. 출처 칸의 옛 ID(M5-17 등)로 코드 주석에서 찾는다 |
| [evidence.md](rust/evidence.md) | 지금 코드가 기대는 측정과 시험 결과, 측정 방법 |
| [backlog.md](rust/backlog.md) | 열린 일: 출시 전 할 일, 열린 gate, 미룬 결정, 알려진 한계, 개선 후보 |
| [adr-operations-and-format.md](rust/adr-operations-and-format.md) | 편집 연산의 의미와 `.ugurugu` 형식 |
| [ugu-import.md](rust/ugu-import.md) | 2.x `.ugu` schema 13 가져오기 대응표 |

새 결정은 decisions.md에, 새 측정은 evidence.md에, 열린 일은 backlog.md에 적는다. 같은 내용을 여러 문서에 다시 쓰지 않는다. 일을 닫으면 backlog.md의 행과 다른 문서의 그 ID도 함께 지운다. CI의 `tools/check_docs.py`가 문서 링크와 backlog ID를 검사한다.

## 지운 문서

2026-10-12에 마일스톤 계획(`RUST_WINDOWS_PORT_PLAN.md`, `rust/m0-close.md`, `rust/m2-plan.md`~`m6-plan.md`), 옛 측정 기록(`rust/m0-evidence.md`), 2.x 상태 문서(`project-status.md`), 검토 보고서(`ANALYSIS_REPORT.md`, `analysis-3.0.md`, `RUST_3_REVIEW_2026-10-12.md`)와 그 원자료(`review-evidence-*`)를 지웠다. 남길 것은 위 문서로 옮겼고, 원문은 git `70f9454`에 있다(`git show 70f9454:docs/rust/m6-plan.md`).

2026-10-13에 문서 탭·UI 다듬기 계획(`rust/plan-document-tabs.md`)을 지웠다. 결정은 decisions.md, 측정은 evidence.md 36~43절, 남은 일은 backlog.md로 옮겼고, 원문은 git `70c27f4`에 있다.

2026-10-14에 3.2.0 애니메이션 계획(`rust/plan-animation.md`)을 지웠다. 결정은 decisions.md 4절 "키프레임과 애니메이션 폴더"와 ADR 8절, 측정은 evidence.md 55·56절, 남은 일은 backlog.md 12절로 옮겼고, 원문은 git `d0506ba`에 있다.

## 그 밖

사용자 안내는 루트 README(ko·en·ja), 빌드는 BUILDING.md, 기여는 CONTRIBUTING.md, 보안은 SECURITY.md, 판별 변경 내용은 `release-notes/`에 있다. 제3자 고지는 저장소에 두지 않고 패키지를 만들 때 `tools/third_party_notices.py`가 `THIRD_PARTY_NOTICES.txt`로 만든다.
