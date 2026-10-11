# Ugurugu 3.0 검토 근거

기준은 2026-10-12 한국 시간, `rust/m6`의 `03ef8a824db38632346c9c4b3310b7ade1119f1c`다. [보고서](C:/Users/Nyabi/Documents/Code/Ugurugu/docs/RUST_3_REVIEW_2026-10-12.md)의 U3 항목을 재현하고 검사 결과를 보존한다. 웹과 C++ 2.x 시험은 실행하지 않았다.

## 검사 기록

| 파일 | 내용 |
|---|---|
| `metadata.json` | 소스 기준·규모·크레이트별 줄 수·기존 시험 집계 |
| `cargo-test-unrestricted.log` | 기존 Release 시험 485 통과·3 제외 |
| `cargo-test.log` | 최초 sandbox 실행. 파일 교체 권한 오류로 중단돼 최종 결과로 쓰지 않음 |
| `cargo-fmt.log` | 정상 권한 형식 검사 성공. 출력이 없어 빈 파일 |
| `cargo-clippy.log` | Clippy 경고 오류화 검사 성공 |
| `cargo-build.log` | workspace Release 빌드 성공 |
| `cargo-deny.log` | 캐시된 권고 DB로 오프라인 검사 성공. 중복 버전 경고 포함 |
| `native-deps.log` | 네이티브 의존 정책 통과 |
| `review-probes.log` | U3-01~07·12 관찰 시험. UI는 한국어 기본 배치의 접근성 좌표 |
| `probe-runner.log` | 일시 삽입한 소스를 원래 바이트로 복원한 SHA-256 |
| `fixture-1.log`, `fixture-3.log` | 생성한 벤치마크 문서 구성 |
| `render-1.log`, `render-3.log` | 8작업자 CPU 렌더 측정 |
| `io-bench.log` | 긴 획 문서의 5회 입출력 측정 |

## 결함 재현

저장소 루트에서 실행한다. 관련 소스에 동시 편집이 없어야 한다.

```powershell
python docs/review-evidence-2026-10-12/run-probes.py
```

이 도구는 `files.rs`, `ugu-session/src/lib.rs`, `update.rs`, `ui/window.rs`, `settings.rs`에 해당 `*-probes.rs`의 테스트 모듈을 일시 추가한다. Cargo로 `review_audit` 테스트만 실행한 다음 `finally`에서 원래 바이트로 복원한다. 기존 변경이 있는 파일은 거절하고, 실행 중 다른 변경이 발견되면 원본 백업을 남겨 덮어쓰지 않는다. 네이티브 창, 실제 사용자 설정, 실제 클립보드를 변경하지 않는다. 파일 시험은 고유한 임시 디렉터리에서 수행한다.

**이 시험의 통과는 결함이 현재 존재함을 확인한 결과다.** 수정 후 유지할 회귀 시험으로 쓰려면 “작업 보존·경로 불변·복구본 유지”가 성공 조건이 되도록 assert를 바꿔야 한다. U3-08~11은 이 실행 로그로 재현했다고 주장하지 않는다.

강제 프로세스 종료나 PC 종료는 Python의 `finally` 실행을 막을 수 있으므로 실행 중에는 소스 편집과 다른 Cargo 작업을 피한다.

## 성능 측정 재현

```powershell
cargo build --workspace --release --locked --offline
target/release/ugu-doc.exe fixture 1 docs/review-evidence-2026-10-12/fixture-1.ugurugu
target/release/ugu-doc.exe fixture 3 docs/review-evidence-2026-10-12/fixture-3.ugurugu
target/release/ugu-doc.exe render docs/review-evidence-2026-10-12/fixture-1.ugurugu 8
target/release/ugu-doc.exe render docs/review-evidence-2026-10-12/fixture-3.ugurugu 8
target/release/ugu-doc.exe bench docs/review-evidence-2026-10-12/fixture-3.ugurugu 5
```

생성한 fixture는 재생성 가능하므로 보고서 산출물에는 로그와 명령을 남긴다. 측정은 Intel Core Ultra 7 270K Plus의 Windows x64에서 실행했으며, 전체 앱의 입력 지연·GPU 표시·저사양 성능 측정은 아니다. 기존 M0 성능 수치를 이번 실행 결과로 재사용하지 않았다.
