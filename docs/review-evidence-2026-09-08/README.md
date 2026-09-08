# 검토 증거 사용 안내

대상 커밋은 `review-metadata.json`에 기록했다. 상위 폴더의 `project-review-2026-09-08.md`가 결과와 제한을 설명한다.

- `ctest-release.log`, `native-release-build.log`: 이번 로컬 실행 로그.
- `checks-summary.txt`: 도구 출력에서 옮긴 결과 요약. 별도의 재실행 로그가 아니다.
- `probe.cpp`, `run-probe.py`, `probe.log`: 빌드된 실제 네이티브 라이브러리를 재사용한 집중 재현. 설정과 복구 경로는 임시 디렉터리로 격리한다. 기본 build 위치와 검토 당시 절대 경로가 포함되어 있다. 다른 checkout에서 실행하려면 해당 경로를 조정해야 한다.
- `native-*.png`: 위 probe의 Qt/Fusion/offscreen 화면. 실제 Cocoa/Windows GPU 표시 결과는 아니다.
- `web-boundary-reproduce.mjs`: 실제 제품 함수를 추출해 Worker·IndexedDB·DOM 대역과 실행한 네 가지 문제 재현. Node 24 기준이며 저장소 경로가 코드에 기록되어 있다. **이 스크립트의 assertion 성공은 현재 결함의 재현 성공을 뜻한다.** 수정 후 회귀 테스트로 사용하려면 기대값과 테스트 구조를 바꿔야 한다.
- `qt-transform-and-decompression.log`, `raster-budget-probe.log`: Qt 계산 및 원본 래스터 처리 경로의 제한된 합성 입력 실험 결과. 실제 메모리 고갈이나 전체 악성 문서 로딩을 재현한 결과가 아니다.

제품 소스 변경 없이 검토 목적으로 작성한 자료다. 네이티브 UI probe는 테스트 접근자를 통해 상태를 준비하므로 모든 사례가 화면 클릭부터 시작하는 E2E 테스트인 것은 아니다.
