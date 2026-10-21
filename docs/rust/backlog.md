# 3.x 백로그

2026-10-12 정리, 2026-10-21 갱신. 3.x(Rust, `apps/ugurugu`, `crates/*`)에 아직 열려 있는 일을 모은다. M0~M6 계획과 검토 보고서를 지우기 전에 거기서 뽑았다. 지운 문서는 git `70f9454`에 있다(`git show 70f9454:docs/rust/m6-plan.md` 등). 출처 칸의 줄 번호는 그 커밋 기준이다. 어디 칸은 줄 번호가 아니라 함수 이름으로 가리킨다(줄 번호는 코드가 바뀌면 어긋난다).

- C++ 2.x와 웹 판은 더 유지하지 않는다(사용자 결정). 2.x 전용 항목과 2.x 릴리스는 버렸다. 2.2.13 소스는 `v2.2.13` 태그에만 있고(2026-10-13 저장소에서 지움), 비교 기준과 `.ugu` fixture 생성기가 필요하면 그 태그를 빌드한다.
- 각 항목은 지금 코드에서 싸게 확인할 수 있는 것은 확인했다. 확인하지 않은 영향·원인은 **가설**로 적는다.
- ID `B3-nn`은 이 문서 전용이다. 원래 문서의 ID(U3-xx, A-xx, B-xx 등)는 출처 칸에만 쓴다.

## 1. 사용자 할 일

| ID | 할 일 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-142 | 간체 중국어 번역을 읽어 본다 | 2.2.13 번역이 없어 새로 옮겼고 중국어를 읽는 사람의 검토를 받지 않았다(decisions.md 9절). 용어와 어색한 문장을 고친다 | `apps/ugurugu/i18n/strings.txt`의 다섯째 열 | 2026-10-16 간체 중국어 |

## 2. 열린 gate

출시를 막지 않는다. 통과로 적지 않고, 릴리스 노트에 시험한 범위만 쓴다.

| ID | gate | 지금 근거 | 닫는 방법 | 출처 |
|---|---|---|---|---|
| B3-06 | 실제 펜·터치 장치 | Windows Sandbox 합성 입력(`InjectSyntheticPointerInput` PT_PEN·PT_TOUCH): 압력·barrel·hover·창 밖 해제·1000개/초, 두 손가락 팬·핀치·회전. 지우개 끝은 Windows가 합성 플래그를 넘기지 않아 단위 시험만(`canvas.rs` `a_pens_eraser_end_erases_whatever_tool_is_chosen`) | 태블릿 드라이버에서 **Windows Ink를 켠다**(3.0은 Windows Pointer만 읽고 WinTab은 읽지 않는다). 압력 곡선, 지우개 끝, barrel, hover 링, 창 밖 해제, 터치 두 손가락 제스처와 펜 우선을 확인한다. 같이 볼 가설: `frameId`·`PerformanceCount`를 0으로 보내는 장치는 샘플 순서가 뒤집힐 수 있다. 닫으면 릴리스 노트의 펜 문장을 고친다 | m6-plan 2절 10, 6절 M6-9·M6-10, 7절, 8절 |
| B3-07 | RTX 5080·WARP 밖의 GPU | 이 PC의 RTX 5080(DX12)과 `UGURUGU_ADAPTER=warp`. GPU가 없는 경우의 자연 WARP 전환은 CI smoke(windows-2025)만. 하드웨어 실패 → WARP(`gpu::with_warp_fallback`)는 Sandbox가 호스트 GPU를 보여 줘서 실제로 만들지 못했다. Intel iGPU·AMD는 고를 수 없었다 | 사용자 보고(로그의 `selected GPU adapter`, 정보 탭의 진단 정보 복사). 다른 PC에서 실행하거나 B3-10의 진단 옵션을 만든 뒤 iGPU로 실행 | m6-plan 7절, 8절, analysis-3.0 A-03·6절 |

## 3. 출시 뒤로 미룬 결정

| ID | 항목 | 왜 미뤘나·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-08 | M6-3 시작 어댑터 나열 약 0.3초 | wgpu `request_adapter`가 어댑터 4개의 드라이버를 모두 연다(어댑터마다 첫 D3D12 장치 110~170ms). wgpu 30에는 하나만 여는 공개 API가 없다(`dx12::Adapter::expose` 비공개). fork·패치는 하지 않기로 했다. 창이 뜬 뒤라 체감은 첫 프레임 약 0.48초의 일부. wgpu를 올릴 때 다시 본다 | `crates/ugu-render/src/gpu.rs` | m6-plan 2절 3, 3절, M6-3, m5-plan M5-2 |
| B3-09 | M6-12 2.2.13 ③ 채우기 클릭→화면 측정 공백 | 2.2.13 값을 재지 못했다(측정 도구 scratchpad `fill_probe.py`, 사용자 PC 사용으로 값 버림). 2.x를 더 유지하지 않으므로 비교 기준으로만 의미가 있다. 버려도 된다 | GUI 측정 | m6-plan 3절, 6절 M6-12 |
| B3-10 | Intel iGPU 등 어댑터를 고르는 진단 옵션 | 지금 `UGURUGU_ADAPTER`는 `warp`만 안다. B3-07을 이 PC에서 닫으려면 필요하다 | `crates/ugu-render/src/gpu.rs` `from_env` | m6-plan 4절 순서, 7절 |
| B3-11 | 델타 업데이트 | 2.2.13도 `--delta None`. 전체 패키지 15.5MB | `tools/package_windows.ps1` | m6-plan 1절 |
| B3-12 | 코드 서명 | 서명 없이 내기로 했다(2026-10-11 사용자). SmartScreen 경고는 도움말에 있다. 서명하게 되면 `vpk pack --signParams`만 더한다. 2.x 분석 R-02의 지적도 그대로 해당한다: Windows 업데이트의 신뢰 근거가 GitHub 릴리스 하나뿐이라 계정·토큰이 털리면 업데이트 경로로 코드가 나간다(서명 전까지는 2FA·토큰 범위 관리) | `tools/package_windows.ps1`, `release-3.yml` | m6-plan 2절 7, 5절; ANALYSIS_REPORT R-02 |
| B3-13 | macOS 3.x | 3.0은 Windows만. 2.x 릴리스 workflow(`release.yml`, macOS 서명·공증·Sparkle 포함)는 지웠다. 3.x가 macOS를 지원할 때 새 비밀(인증서·공증·업데이트 키)로 다시 쓴다. 코드 쪽은 `ugu-render`가 CPU 렌더와 DX12 표시를 함께 의존하고 Windows FFI가 `ugu-win`에 모여 있어, 렌더·표시 경계를 먼저 나눠야 한다 | `.github/workflows/`, `crates/ugu-render/Cargo.toml`, `crates/ugu-win` | m6-plan 2절 14, analysis-3.0 3.1·4절 14 |
| B3-14 | macOS 2.2.13의 업데이트 확인 | 2.2.13의 Sparkle 피드는 `releases/latest/download/appcast.xml`이다. 3.x가 GitHub Latest라 이 주소는 404다(2026-10-13 `curl`로 확인, `v3.0.1`로 리디렉션된 뒤 404). m6-plan은 "업데이트 없음"으로 끝난다고 적었지만 2.2.13에서 확인하지 않았다(**가설**). Sparkle이 404를 오류로 보여 줄 수도 있다. 자동 확인이면 조용히 실패하고 수동 확인이면 오류 창이 뜰 것으로 보인다(가설). 피하려면 3.x 릴리스마다 2.2.13의 `appcast.xml`을 자산으로 함께 올리는 방법이 있다(정하지 않음) | GitHub Releases, `release-3.yml` | m6-plan 2절 14 |
| B3-15 | ARM64 Windows | 범위 밖으로 뺐다. 빌드 대상·libwebp·ring·Velopack 런타임을 함께 봐야 한다 | `.cargo/config.toml`, 패키지 | m6-plan 1절, 8절 |
| B3-78 | 문서 탭을 창 밖으로 떼어 새 창 만들기 | 3.0 탭은 순서 바꾸기만 한다(사용자 결정 2026-10-12). 떼려면 문서를 다른 프로세스로 넘겨야 한다 | `apps/ugurugu/src/render.rs` | [plan-document-tabs.md](https://github.com/nyabi-gh/Ugurugu/blob/70c27f4/docs/rust/plan-document-tabs.md) 3절 3 |
| B3-80 | 탭이 많을 때 보이는 탭의 표면 예산 | 숨은 탭은 레이어 표면을 놓지 않고(⑤ 탭당 약 405MiB), 표면 예산은 탭마다 빈 메모리로 정한다. 메모리가 적은 PC에서 큰 문서 탭이 많으면 보이는 탭이 바닥값(128MiB)으로 내려가 레이어를 하나씩 그릴 수 있다(가설, 8GiB PC에서 재지 않음). 고치려면 숨은 탭 작업자에 표면을 비우는 일을 보내고 예산을 창 전체에서 보이는 탭 먼저 나눈다. 탭마다 래스터 스레드 풀(8개)을 따로 두는 것도 함께 볼 수 있다(쉬는 스레드라 비용은 측정상 없음) | `apps/ugurugu/src/cache.rs`, `budget.rs`, `canvas.rs` | evidence.md 41절, plan-document-tabs 4절 5 |
| B3-82 | 일본어 줄바꿈 금칙 | egui는 글자 단위로 줄을 바꾸므로 작은 가나·장음·구두점이 줄 머리에 올 수 있다(설명 글 몇 곳). 넘치지는 않는다 | egui 줄바꿈 | evidence.md 43절 |
| B3-83 | 날짜 표기의 언어 | 복구 안내 등의 날짜는 시스템 로캘로 쓴다. UI 언어가 다르면 섞인다(한국어 Windows + 일본어 UI에서 "오전") | `ugu-win` `locale` | evidence.md 43절 |
| B3-79 | egui 기본 모양이 남은 곳 | 드롭다운의 닫힌 버튼(`egui::ComboBox`)은 egui 것을 팔레트로 칠해 쓴다(높이 28·모서리·색은 테마 값, 화살표는 egui 삼각형). 다른 위젯과 어긋나는지는 사용자가 Screen set 화면으로 본다. 선 속성의 색 버튼은 `6b2f565`에서 앱의 색 견본(`widgets::color_swatch`)으로 바꿈 | `ComboBox` 쓰는 곳 | 2026-10-12 위젯 통일, evidence.md 42절 |

## 4. 알려진 한계

기록된 차이다. 고칠 계획은 없고, 필요하면 개선 후보로 옮긴다.

| ID | 한계 | 영향·이유 | 어디 | 출처 |
|---|---|---|---|---|
| B3-16 | 변형 상자 밖(회전 자리)의 커서가 화살표 | egui `Alias`를 winit이 화살표로 바꾼다. 2.2.13에는 회전 손잡이가 없어 비교 대상이 없다 | `apps/ugurugu/src/canvas/transform.rs` | m6-plan 7절 GUI 확인, 8절 |
| B3-17 | 파일을 열 때 내장 이미지를 끝까지 디코드해 검증 | 4096² 한 장에 최대 약 50ms(압축 잘 되는 그림 47ms, 잡음 18ms) 더 걸린다. 손상 PNG를 정상처럼 여는 것(A-01)을 막는 비용 | `crates/ugu-io/src/read.rs` `draws_as` | m6-plan 6절 분석 보고서 확인, 8절 |
| B3-18 | RDP는 Windows Sandbox 원격 세션까지만 확인 | Sandbox도 `TerminalServerSession`이지만 `mstsc`로 다른 PC에 접속한 실행과 같다고 보는 것은 가설 | — | m6-plan 7절 |
| B3-19 | 업데이트 경로 중 시험하지 않은 것 | GitHub `updates` 릴리스에서 받는 실제 업데이트(`HttpSource`)는 3.0.0 → 3.0.1에서 확인했다(evidence.md 46절). 설치 뒤 적용 실패(`update-install-failed`)는 만들어 내지 못했다. 여러 창이 떠 있을 때 갱신기가 다른 창을 어떻게 닫는지, 바이러스 백신이 있는 실제 PC도 보지 않았다 | `apps/ugurugu/src/update.rs` | m6-plan 6절 M6-5·M6-7 결과, 7절, 8절 |
| B3-20 | 그리는 중 픽셀 획은 프레임마다 전체를 다시 계산 | 3초 빠른 획 끝에서 약 1.5ms/프레임(잉크 펜의 10배 이상). 프레임을 놓치지 않아 그대로 둔다. B3-24와 관련 | `crates/ugu-render/src/pixel.rs` | m6-plan 6절 M6-16, 8절 |
| B3-21 | 진행 중 dab 획은 표본 상한(`MAX_DABS` 5만)을 모르고 그린다 | 상한을 넘는 긴 에어브러시·스프레이 획은 확정 뒤 간격이 넓어진 모양으로 바뀐다. 기록 당시 사실이고 지금도 같은지는 다시 확인하지 않았다(가설) | `crates/ugu-render/src/dab.rs` | m4-plan 603행 |
| B3-22 | Windows 10은 따로 시험하지 않음 | 막지 않고 "Windows 10 이상"을 둔다(사용자 결정). Sandbox 게스트는 Windows 11 계열(10.0.26100) | — | m6-plan 2절 12 |
| B3-143 | 번체 중국어 Windows는 영어(또는 다음 선호 언어)로 보임 | 중국어 번역은 간체 하나다(사용자 결정 2026-10-16). 번체 표시(`Hant`·`TW`·`HK`·`MO`)가 있으면 `catalogue`가 번역이 없다고 답해 다음 선호 언어, 없으면 영어로 간다. 번체 번역을 더하려면 문자열 열과 글꼴(번체 자형)을 함께 정해야 한다 | `apps/ugurugu/src/i18n.rs` `catalogue` | 2026-10-16 간체 중국어 |

"그리는 동안에도 우글거림 유지" 선택지가 없는 것도 한계로 기록돼 있었다. B3-23으로 옮긴다.

## 5. 개선 후보 (기록된 성능·기능 후보)

| ID | 후보 | 왜·기대 | 어디 | 출처 |
|---|---|---|---|---|
| B3-23 | 설정 "그리는 동안에도 우글거림 유지" | 2.2.13에 있던 선택지. 3.0은 2.2.13 기본값(획 동안 멈추고 끝나면 다시 재생)만 옮겼다. 원하면 넣는다 | `apps/ugurugu/src/canvas.rs` `pause_playback`, 설정 그리기 탭 | m6-plan 2절 22, 8절 |
| B3-24 | 픽셀 획을 Vello 없이 레이어 픽셀에 직접 쓰기 | 너비 12 픽셀 획은 1스레드 1.5배·8스레드 1.4배(계단 경계를 Vello가 채우는 비용). 확정 스탬프는 이미 직접 쓴다. 렌더 단계 순서와 축소 렌더를 다시 짜야 한다 | `crates/ugu-render/src/pixel.rs`, `document.rs` | m5-plan 115행(M5-16), m0-evidence 966행 |
| B3-25 | 픽셀 칸이 그대로인 프레임은 다시 그리지 않기 | 재생은 첫 바퀴 뒤 프레임을 모두 쥐므로 첫 바퀴에만 이득 | `crates/ugu-render` | m5-plan 115행 |
| B3-26 | 큰 타원 선택 회전에서 출력 행마다 구간 모으기 | 32 → 20ms까지 줄였고 더 줄일 여지 | `crates/ugu-core` 선택 마스크 | m5-plan 117행(M5-12) |
| B3-27 | 선택 끌기 미리보기를 만드는 비용 | 누를 때 62~69ms(새 픽스맵 8장 할당과 평균), 갱신은 2048² 합성 5ms라 160Hz(6.25ms)에 못 미친다. 후보: 같은 대기 변형 안에서 다시 끌 때 재사용, 남김·뺌을 한 번에 평균, 원본 부분을 남긴 것에서 잘라 쓰기 | `apps/ugurugu/src/canvas/transform.rs` | m5-plan 117행, m0-evidence 880행 |
| B3-28 | 획 윤곽을 묶음으로 흘려 보내기 | 1스레드 렌더가 레이어 전체 윤곽을 쥔다(③ 39, ④ 59MiB). 윤곽 만들기와 그리기 사이 동기화가 늘어난다 | `crates/ugu-render` | m5-plan 118행(M5-13) |
| B3-29 | 큰 텍스처 업로드를 프레임 인코더에 합치기 | 지금은 큰 업로드마다 따로 submit·poll해 render 스레드가 0.25ms 더 쓴다 | `crates/ugu-render/src/view.rs` | m5-plan 118행 |
| B3-30 | 패널을 메인 창 밖(두 번째 모니터)으로 떼기 | 자체 도킹은 떠 있는 패널도 메인 창 안이다. 2.2.13 Qt 도크와 남은 차이 | `apps/ugurugu/src/layout.rs`, `ui/dock.rs` | m5-plan 119행(M5-11) |
| B3-31 | 축소 내보내기를 작게 그린 뒤 줄이기 | 작은 크기에서는 시간·메모리 대부분이 원래 크기 그리기다. 다만 2.2.13 NativeExact(원래 크기로 그린 뒤 면적 평균)와 화소가 달라져 하지 않았다. 차이를 받아들일 때만 | `apps/ugurugu/src/export.rs` | m5-plan 120행 |
| B3-32 | 100% 이상으로 큰 문서를 재생할 때 보이는 영역만 그리기 | 프레임이 화면 크기로 줄어 예산이 모자란 PC의 재생이 빨라진다 | `apps/ugurugu/src/cache.rs` | m4-plan 652행 |
| B3-33 | egui-wgpu 장치 손실 panic 우회 제거 | egui-wgpu가 손실된 장치에서 오류 대신 panic하므로 장치 손실 때만 `catch_unwind`로 흡수한다. 상류가 오류를 돌려주게 고쳐지면 지운다 | `apps/ugurugu/src/render.rs` `RenderThread::frame`의 `catch_unwind` | m0-close 58행 |
| B3-34 | egui TextEdit의 UIA `SetValue` | egui가 지원하지 않아 일부 보조 기술이 제한된다. 수용했고 egui를 올릴 때 다시 본다 | egui | m0-close 64행 |
| B3-35 | Pedantic Clippy 정리 | workspace 정책은 `clippy::all = warn`. pedantic 경고 블록 1,749개(`cast_possible_truncation` 433 등). 좌표·바이트·픽셀 변환의 범위 검증부터 | `Cargo.toml` `[workspace.lints]` | analysis-3.0 3.4, 4절 13, 6절 |
| B3-130 | Sandbox 둥근 픽셀 점 단계의 확대율 | "Round pixel dots at several zooms"가 확대율 칸에 100을 넣었는데 500%로 찍혔다(evidence 60). 단계는 넣은 값을 확인하지 않아 통과했다. 왜 100이 들어가지 않았는지 찾고, 찍기 전에 확대율을 확인하게 한다 | `tools/sandbox/gui.ps1` | evidence 60, 70, 73 |

## 6. 검토 보고서에서 남은 것

`docs/analysis-3.0.md`(이하 분석, `b809009` 기준)와 `docs/RUST_3_REVIEW_2026-10-12.md`(이하 검토, `03ef8a8` 기준)에서 고치지 않은 것. 분석 A-01~A-05와 검토 U3-01~U3-12는 모두 고쳤다(m6-plan 6절).

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-36 | 내보내기의 정적 레이어 재사용 | 내보내기 렌더는 revision에 `None`을 넘겨(`render_plan(..., None, ...)`) 정적 레이어를 다시 쓰지 않는다. 복잡한 정적 배경의 긴 애니메이션은 매 프레임 다시 그린다(비용 크기는 가설). 문서 식별자와 revision을 함께 넘겨야 하고 `None == None`으로 재사용하면 다른 문서가 섞인다 | `apps/ugurugu/src/cache.rs` `run`의 `Job::Export`, `crates/ugu-render/src/document.rs` | 분석 3.3-3, 4절 11 |
| B3-37 | 2.2.13 fixture → 3.0 전체 프레임 비교를 CI에 | 지금 CI는 가져오기 대응·거절만 시험하고(`legacy/tests.rs`), 2.2.13 렌더와의 프레임 비교(`ugu-doc compare`)는 수동이었다(m0-evidence 22절). C++를 더 빌드하지 않으므로 2.2.13 생성기로 기준 프레임 PNG를 한 번 만들어 저장소에 두고 CI가 비교하는 방식이 맞다 | `tests/fixtures/ugu13`, `tools/ugu-doc`, `.github/workflows/ci.yml` | 분석 2.2 |
| B3-38 | 프로세스 전체 메모리 예산과 저메모리·여러 창 측정 | 캐시 `Budget`은 최소 256MiB를 강제하고(가용 100MiB여도), 디코더·히스토리·GPU staging·내보내기·다른 창은 이 예약에 묶이지 않는다. 이미지 디코더 1GiB 한도도 고정값이다. 저메모리·여러 창·큰 내보내기 조합의 OOM·지연은 재현하지 않았다(가설). 잴 조건: 8·16GiB, 큰 문서 탭 1·4·8개, 저장·가져오기·내보내기 조합에서 peak private bytes, commit 여유, 탭 바꾸기 지연, 취소 p95. B3-80과 같이 예산을 한 곳에서 나누고 숨은 탭부터 회수하는 정책을 정하되, 예산 초과 split의 화소 일치 시험(`over_the_budget_the_split_shows_the_bytes_of_a_full_render`)을 지킨 채로 한다 | `apps/ugurugu/src/budget.rs` `FLOOR`, `crates/ugu-io/src/import.rs` `ALLOCATION` | 분석 3.3-1·3.3-5, 4절 12; 검토 8절 |
| B3-39 | 내보내기 대기 상한·공정성 | 캐시 작업자는 편집·재생을 내보내기보다 먼저 하고 내보내기는 `answer.recv()`로 기한 없이 기다린다. 계속 편집할 때 내보내기 완료·취소 정리가 얼마나 늦는지 재야 하고, 늦으면 시간 할당이나 대기 상한을 둔다. 편집 우선 자체는 결함이 아니다. 정상 작업을 실패로 만드는 시간 초과보다, 취소가 대기 중 요청·진행 중 렌더·인코더·파일 정리까지 닿는지와 내보내기의 최소 진행 기회를 먼저 본다. 잴 것: 연속 편집·재생 중 내보내기 완료 시간, 취소→정리 p95(목표 100ms) | `apps/ugurugu/src/cache.rs` `Renders::render`(`answer.recv()`)와 `Queue`의 `exports` | 분석 3.3-4, 4절 11; 검토 8절 |
| B3-40 | 타일이 사용 영역의 경계 사각형 버퍼 | 두 모서리에 작은 표시만 있어도 큰 사각형이 필요하고, 커질 때 이전·새 버퍼가 겹친다. sparse tile map이 아니다(비용은 가설) | `crates/ugu-render/src/tile.rs` | 분석 3.3-2 |
| B3-44 | `derivative` 대체 추적 | Velopack이 끌고 오는 빌드 시점 매크로로, RUSTSEC-2024-0388(관리 중단 공지, 취약점 아님)을 `deny.toml`에서 허용한다. Velopack 쪽 교체를 지켜본다. Velopack main은 2026-10-16에 뺐고 아직 릴리스되지 않았다 | `deny.toml`의 `[advisories] ignore`, `Cargo.lock` | 분석 3.5; m6-plan 6절 M6-5 |
| B3-145 | zip 9로 올리기 | zip 8.6.0에 RUSTSEC-2026-0367·0368·0369(9.0.0에서 고침)가 나와 `deny.toml`에서 허용한다. 셋 다 `ZipArchive::extract`·`ZipStreamReader::extract` 안의 결함인데 ugu-io는 그 함수를 부르지 않고, Velopack 1.2.161도 `by_index`로 읽어 직접 파일에 쓴다(2026-10-17 소스로 확인). Velopack 1.2.161이 zip `^8.6`을 요구해 ugu-io만 올리면 zip이 두 판 들어가고 8.6.0은 그대로 남는다. Velopack main은 2026-10-16에 zip 9.0.3으로 올렸다("all 8.x releases are yanked"). 그 Velopack이 나오면 둘을 함께 zip 9로 올리고 허용 세 줄을 지운다 | `Cargo.toml` `zip`, `deny.toml`, `crates/ugu-io` | 2026-10-17 `cargo deny` |
| B3-45 | egui-winit을 올릴 때 `EguiState` 수동 `Send` 재검토 | "AccessKit adapter가 비어 있으면 옮겨도 된다"는 가정에 기댄다. 지금 판에서 문제를 재현한 것은 아니다 | `apps/ugurugu/src/render.rs` `EguiState`의 `unsafe impl Send` | 분석 3.2; 검토 9절 |
| B3-46 | GPU 시험이 어댑터가 없으면 조용히 통과 | `view.rs`의 GPU 시험은 렌더를 얻지 못하면 "skipped: no DX12 adapter"를 찍고 `return`한다. 전체 통과가 GPU 표시를 보증하지 않는다. CI에서 WARP로 0개 실행이면 실패로 하는 방법이 있다 | `crates/ugu-render/src/view.rs` `tests::device`와 `render`·`render_grid`를 쓰는 시험(`at_whole_scales_each_screen_pixel_shows_one_document_pixel` 등) | 분석 3.4 |
| B3-47 | 코드에 직접 넣은 사용자 문구 검사 | Fluent 키 일치 시험은 언어 파일끼리만 비교해 코드의 하드코딩 영어를 찾지 못한다. 지금 남은 하드코딩 문구가 있는지는 전수 확인하지 않았다(가설) | `apps/ugurugu/src/i18n.rs` | 분석 3.7; 검토 6절 |
| B3-50 | 긴 텍스트의 윤곽을 UI 스레드에서 다시 만듦 | 배치한 텍스트가 바뀌면 render/UI 스레드에서 Parley 윤곽을 다시 만든다(같은 글이면 캐시). 긴 문장·대량 붙여넣기는 입력 지연 후보. 재고 길이 제한·지연 합치기·백그라운드 배치를 검토 | `apps/ugurugu/src/canvas/text.rs` `Typesetter::outline` | 검토 8절 |
| B3-51 | 축소 재생 미리보기의 정확성 fixture | 재생 때 `preview_shrink`는 해상도·표본 간격을 줄인다. 픽셀 모드와 곡선의 축소 미리보기 비교 fixture가 있으면 좋다. 릴리스 노트 문구는 고쳤고 fixture는 더하지 않은 것으로 보인다(가설) | `apps/ugurugu/src/canvas.rs` | 검토 8절, 10절 |
| B3-52 | 작업 흐름 안내 | 도움말에는 "시작하기"가 있지만 첫 실행에 띄우지 않는다. 문자 안내(`text-hint`)는 확정하면 윤곽이 되어 다시 편집할 수 없다는 말이 없다. 삽입한 이미지를 캔버스에 맞춰 줄여도 "삽입됨"만 알린다(`import::decode`의 `fit`). 메뉴의 "현재 프레임을 이미지로 내보내기"와 동작 이름 "이미지로 내보내기"가 다르다. 움직이는 GIF·WebP를 삽입하면 첫 프레임만 가져오지만(`import.rs`) 그렇다고 알리지 않는다. 확인 창을 늘리기보다 결과가 바뀌는 자리(문자 패널, 실제로 줄였을 때의 알림)에 짧게 둔다. 문구 예: "적용하면 글자가 선으로 바뀝니다", "이미지를 6000×4000에서 1024×683으로 줄여 넣었습니다"(숫자는 실제 값) | `apps/ugurugu/src/ui/`, `files.rs`, `i18n/strings.txt` | 검토 6절 "작업 흐름 개선 제안", 3.1.0 종합 검토 |
| B3-53 | PNG 연속 프레임·sprite sheet 내보내기 | 게임 엔진·영상 편집으로 재사용, 픽셀 모드와 잘 맞는다 | `apps/ugurugu/src/export.rs` | 분석 3.8 |
| B3-54 | 폴더 밖 레이어의 어니언 스킨 | 3.2.0의 어니언 스킨은 애니메이션 폴더와 그 셀에서만 켜진다(`Canvas::onion_wanted`는 `session.folder()`가 없으면 `None`). 보통 레이어에서 앞뒤 프레임을 겹쳐 우글거림 진폭·끊김·연결을 정지 상태에서 비교하는 것은 아직 없다. `RenderPlan::alone`은 레이어 id 하나를 그 프레임의 자세로 그리므로 폴더가 아닌 레이어에도 쓸 수 있어 보이나 확인하지 않았다(가설). 키프레임 편집기 전체가 아니라 포즈 비교로 한정 | `apps/ugurugu/src/canvas/onion.rs` `onion_wanted`, `crates/ugu-render/src/plan.rs` `RenderPlan::alone` | 분석 3.8, decisions.md 4절 |
| B3-55 | 모션 seed 다시 뽑기·잠금, A/B 비교 | 같은 설정에서 다른 느낌을 찾고 고른 결과를 재현 | `crates/ugu-core/src/store.rs`, `apps/ugurugu/src/ui/mod.rs` | 분석 3.8 |
| B3-136 | 내보내기 미리보기와 프리셋 | 애니메이션 창은 크기·인코딩 메모리·재생 길이·GIF 가장자리 안내를 보인다(3.3.0). 최종 인코딩과 같은 옵션으로 바꾼 한 프레임 미리보기와, 최근 설정이나 이름 있는 몇 개의 내보내기 프리셋은 없다 | `apps/ugurugu/src/files.rs` 애니메이션 창, `export.rs` | 분석 3.8, 3.1.0 종합 검토 |

## 7. 2.2.x 분석 보고서 중 3.0에도 해당하는 것

`docs/ANALYSIS_REPORT.md`(C++ 2.2.x 분석)의 항목 가운데 Rust 코드에서 같은 문제를 확인한 것만 옮겼다. 대부분은 해당하지 않는다. 예: B-01(위가 클리핑 레이어면 병합 거절, `command.rs` `merge_down`), B-06(접촉 중 barrel은 획을 이어 감, `input.rs`), B-07(펜이 떠나도 키 상태 유지), U-02(`Ctrl+Shift+Z` 있음), U-03(파일 연결·여러 창), R-01(이미지 형식 모두 디코드), B-14(500단계·256MiB). R-02는 B3-12에 합쳤다. R-08(읽기 퍼징)은 3.3.0에서 닫았다(evidence.md 65절).

| ID | 항목 | 왜 3.0에도 해당하나 | 어디 | 출처 |
|---|---|---|---|---|
| B3-60 | 임시 파일을 만들 수 없는 폴더에는 저장할 수 없음 | 저장은 같은 폴더의 `.<이름>.<pid>-<n>.tmp`를 거쳐 교체한다. 직접 쓰기 fallback이 없다. 그런 폴더(ACL·일부 공유)가 실제로 문제인지는 가설이고, fallback은 원자성을 잃으므로 하려면 사용자에게 묻는 편이 맞다 | `crates/ugu-io/src/save.rs` `temporary_path` | ANALYSIS_REPORT B-25 |
| B3-62 | 고대비·글자 크기 설정 없음 | 테마는 어둡게·밝게·Windows 설정과 같게 중에서 고르지만(decisions.md 9절) Windows 고대비 색은 따르지 않는다. 본문 12·작은 글자 11포인트 고정. 시험용 `UGURUGU_UI_SCALE`(egui zoom, decisions.md 7절)이 이미 있어 UI 크기를 설정으로 내기는 쉽다(2026-10-21 UI 검토) | `apps/ugurugu/src/theme.rs`, `app.rs` | ANALYSIS_REPORT U-08; 검토 6절 |
| B3-63 | 필압 곡선·최소값과 입력 장치 상태 표시가 없음 | 필압은 켬·끔(`tools.tablet.pressure`)뿐이다. 필압이 먹지 않을 때 UI에서 펜 감지·압력 지원 여부를 볼 수 없다(로그·진단 정보는 있음). B3-06 실기기 확인 때 함께 본다 | `apps/ugurugu/src/settings/tools.rs`, `crates/ugu-win/src/pointer.rs` | ANALYSIS_REPORT U-09 |
| B3-67 | 릴리스 PDB를 보관하지 않음 | Release는 `debug = "line-tables-only"`로 PDB를 만들지만 패키지는 exe·LICENSE·고지 세 파일만 담고 `release-3.yml`도 PDB를 올리지 않는다. 네이티브 충돌 기록(모듈+오프셋)을 기호로 풀 수 없고, panic backtrace에도 함수 이름이 빠질 것으로 보인다(가설) | `Cargo.toml` `[profile.release]`, `tools/package_windows.ps1`, `.github/workflows/release-3.yml` | ANALYSIS_REPORT R-11 |
| B3-68 | 릴리스를 초안 없이 만든 뒤 자산을 올림 | `softprops/action-gh-release`가 곧바로 공개 릴리스를 만들고 파일을 올린다. 그 사이 `releases/latest/download/…` 같은 링크가 자산 없는 릴리스를 볼 수 있다(창의 길이·영향은 가설). 앱의 업데이트 피드는 해당하지 않는다: 3.0.1부터 고정 사전 릴리스 `updates`에서 읽고 그 패키지는 이 단계보다 먼저 올라간다(decisions.md 10절). `draft: true`로 만들고 올린 뒤 공개 | `.github/workflows/release-3.yml`의 `softprops/action-gh-release` 단계 | ANALYSIS_REPORT R-12 |

## 8. 측정·확인 공백

재지 못했거나 원인을 나누지 못한 것. 근거는 `evidence.md`의 해당 주제.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-69 | WARP 지연의 원인을 나누지 못함 | WARP에서 입력이 60~75ms 늦게 보이고 단계 20~32%가 합쳐진다. 렌더 시간인지 합성 대기인지 나누지 않았다 | `crates/ugu-render/src/gpu.rs` | evidence.md 장치 손실·TDR·WARP |
| B3-70 | ② 8 작업자에서 30fps 미달 | 8스레드 프레임 ② 42~49ms, ②p 35.4~35.6ms(evidence.md 13·14절). 사용자가 받아들였지만 목표는 열려 있다(decisions.md 1절). 다음 수단은 인코드·래스터 일 줄이기(가설) | `crates/ugu-render/src/document.rs` | evidence.md 작업자 수와 레이어 배분 |
| B3-71 | CPU 제조사가 다를 때 픽셀 동일성 | SSE4.2 고정으로 같은 결과가 나온다는 것은 한 CPU에서 SIMD 단계를 바꿔 본 추론이다. 다른 제조사 CPU에서 비교한 적 없다. 또 `f64` `sin`·`cos`·`hypot`은 UCRT가 실행 중 FMA3 판을 고를 수 있어 FMA3 유무에 따라 1ulp 달라질 수 있다(가설, `dab.rs` 스프레이, `stroke.rs`, `motion.rs`). 한 PC 안의 미리보기·최종 동일성과는 무관 | `crates/ugu-render` | evidence.md 렌더러 |
| B3-72 | `vello_cpu` 0.x API 위험 | 0.x라서 판을 올릴 때 API가 바뀔 수 있다. 판 고정과 회귀 시험으로 막고 있다 | `Cargo.toml` | evidence.md 렌더러 |
| B3-73 | 떠 있는 패널의 Tab 순서 | 떠 있는 `egui::Window` 패널 사이의 키보드 이동 순서를 확인하지 않았다 | `apps/ugurugu/src/ui` | evidence.md IME·글꼴·접근성·키보드 |
| B3-76 | 시작 직후 제목의 `*` | 2026-10-12 사용자 실행에서 새 문서 제목이 `제목 없음*`로 떴다. 다음 실행에서는 없었다. 원인 미확인(가설: 복구 또는 픽셀 모드 상태) | `apps/ugurugu/src/files.rs` | 2026-10-12 사용자 실행 |
| B3-119 | `strokes_after_a_crop_or_a_resample_draw_on_the_new_canvas`가 가끔 실패 | `shows_the_document`의 "split 반올림 안 최대 3" 단언이 2026-10-14 로컬 debug 전체 실행(`UGU_WORKER_THREADS=8`), 2026-10-15 main CI(`c6a4e36`, 실행 38691779083), 2026-10-21 PR #120 CI(debug, 실행 39608805791)에서 실패했고, 다시 돌리면 통과했다. 값이 남은 두 번은 모두 차이 255(한 화소가 완전히 다름)라 반올림 차이는 원인이 아니다. 원인 미확인(가설: 캔버스가 보인 split이 비교하는 문서와 다른 판, 곧 자르기 전이나 마지막 획 전의 것). 사용자가 자르기·크기 바꾸기 뒤 잠깐 틀린 그림을 보는지도 모른다. 원인을 찾는다 | `apps/ugurugu/src/files/end_to_end.rs` `shows_the_document` | 2026-10-14 레이어 복제 작업 중 |

## 9. 3.0.0 출시 뒤 코드 검토에서 남은 것

2026-10-13 영역별 코드 검토(읽기만, 일부는 시험 crate로 재현). 고친 것은 3.0.1(`v3.0.1`, 작업 브랜치는 지움)에 들어갔다: 저장 중 원본 손실(`ReplaceFileW` 백업), 대기 변형·글자를 두고 한 레이어·선택·채우기 편집이 변형을 되돌림, 묻는 동안 보이는 탭이 바뀜(엉뚱한 탭 닫기·저장, 내보내기 창 panic), 다른 탭에 열린 파일로 다른 이름 저장, 렌더·내보내기 스레드 panic 뒤 멈춤, 프레임 수 변경 뒤 캐시된 포즈, egui 키보드 UI 확대, release publish 재실행·동시 실행. 아래는 남은 것이다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-86 | 8192px보다 넓은 창 | wgpu 기본 한도(`max_texture_dimension_2d` 8192)로 장치를 연다. 여러 모니터를 하나로 묶은 넓은 화면에서 최대화하면 swap chain 설정이 실패하고 그려지지 않는다(가설, 그런 화면 없음). 어댑터 한도를 요청하고 크기를 한도로 자른다 | `crates/ugu-render/src/gpu.rs`, `present.rs` | 검토 |
| B3-87 | 2.x 가져오기의 clip 마스크 훑기 | 가져오기는 서로 다른 (선택 마스크, `visibilityClip`, 캔버스)마다 그 사각형을 한 번 훑는다. 연산 20,000개가 모두 다른 큰 사각형이면 훑는 화소가 수천억에 이르러 파일 스레드가 오래 막힐 수 있다(가설, 재지 않음). 게다가 clip이 빈 획(선택 마스크가 사각형 안에서 모두 128 미만인 경우 등)은 훑은 뒤 `Importer::op`이 `None`을 돌려줘 연산으로 세지 않는다(`paint`의 `self.operations += 1`은 `Some(op)`일 때만 돈다, 2026-10-16 코드로 확인). 그래서 20,000 상한이 훑는 횟수를 막지 못하고 그런 획의 수는 파일 길이(`FILE_BYTES` 128MiB)로만 제한된다(가설, 재지 않음). 만든 mask 바이트·연산 수·점 수·이미지는 쌓일 때 한도를 본다. 안: 마스크마다 128 이상인 화소의 경계를 한 번만 구해 두고 사각형과 겹치는지 먼저 보거나, 훑은 화소 수에 상한을 둔다 | `crates/ugu-io/src/legacy.rs` `masked`, `bits` | 검토 |
| B3-92 | 펜 위치의 HIMETRIC 환산 | 펜·터치 위치를 `ptHimetricLocation`과 장치 사각형으로 환산하고 `ptPixelLocation`과 비교하지 않는다. 태블릿 영역 매핑·비율 유지·세로 화면에서 어긋날 수 있다(가설). B3-06 실기기 확인 때 본다 | `crates/ugu-win/src/pointer.rs` | 검토 |
| B3-93 | 선택 윤곽 정점 버퍼 한도 | 윤곽 run이 약 1340만 개를 넘으면 `max_buffer_size`(256MiB)를 넘어 그 프레임 전체가 무효가 된다. 4096² 잡음 위 자동 선택 같은 극단만 | `crates/ugu-render/src/ants.rs` | 검토 |
| B3-95 | 같은 레이어가 같은 획 id를 두 번 쓸 때 재스타일 | 선택이 한쪽만 건드리면 `RemoveStroke`가 남은 참조 때문에 실패해 오류가 난다. 그런 상태로 가는 순서는 찾지 못했다(가설) | `crates/ugu-core/src/restyle.rs` | 검토 |
| B3-96 | 채우기 기준 그림과 대기 변형 | 영역 클릭은 이제 대기 변형을 먼저 적용하고 기준 그림을 읽는다. 대기 글자도 같이 적용되므로, 배치 중인 글자를 둔 채 자동 선택을 쓰면 글자가 확정된다. 의도와 맞는지 사용자 확인 | `apps/ugurugu/src/canvas.rs` `click_area` | 검토 |
| B3-97 | 막 뜬 창이 잠깐 UI Automation에 답하지 않음 | Sandbox 시험에서 앱을 띄운 직후 `AutomationElement.FromHandle`이 HRESULT 0x80131505(COR_E_TIMEOUT)로 끝나는 일이 되풀이됐다(evidence.md 45절, 53절, 60절). 시험 스크립트가 곧바로 실패한 것은 Windows PowerShell 5.1이 이것을 `TimeoutException`이 아닌 `COMException`으로 받아서였다(77절, 시험 쪽은 고침). 이제 시험은 다시 묻고, 그렇게 통과한 단계는 "pass after retry"와 무엇을 몇 번 다시 물었는지를 남긴다. 창은 보이지만 그 순간 창 스레드가 메시지를 돌리지 않는 것으로 보인다(가설, 원인 미확인). 화면 읽기 프로그램은 시작 직후 잠깐 창을 읽지 못할 수 있다. "pass after retry"가 어느 단계에서 얼마나 자주 나오는지 모아 원인을 가른다 | `apps/ugurugu/src/app.rs` 시작 순서 | evidence.md 45절, 60절, 67절, 77절 |

## 10. 2026-10-14 외부 검토에서 확인한 것

외부 검토 보고서(`8195424` 기준, 저장소에 두지 않음)의 발견 R01~R10을 코드로 확인했다. R10(evidence.md 39절의 낡은 문장)은 고쳤고, 나머지를 옮긴다. 출처 칸의 R 번호는 그 보고서의 번호다. 재현은 보고서 작성자가 공용 crate를 직접 부른 시험 crate로 했고, 예산 분기는 `set_surface_budget(0)`으로 강제한 것이다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-131 | 못 읽은 복구 폴더의 30일과 시계 | `failed-<유닉스 시각>-…`의 나이는 이름의 시각만 믿는다. 옮길 때 시계가 과거였다가 고쳐지면 다음 시작이 바로 지운다(가설, 재현하지 않음). 파일 시스템 시각도 같은 시계라 그것으로는 고치지 못한다 | `apps/ugurugu/src/recovery.rs` `failed_long_ago` | 3.1.0 출시 전 검토 |
| B3-132 | render 스레드가 창 스레드를 기다리는 남은 길 | 세션이 끝나는 동안 창 스레드는 복구 쓰기를 4초 기다린다. 그때 render 스레드가 장치 손실 뒤 새 표면을 청하고 있으면(`UiEvent::NeedSurface`, 창 스레드만 만든다) 둘 다 4초를 쓴다. 창을 주인으로 하는 대화상자가 그 순간 열리는 경우도 주인 창을 동기 메시지로 끄는지는 보지 않았다(가설). 제목과 포인터는 2026-10-15에 창 스레드로 옮겼다 | `apps/ugurugu/src/app.rs` `ending`, `render.rs` `surface_source` | 3.1.0 출시 전 검토 |
## 11. 3.1.0 종합 검토에서 새로 확인한 것

2026-10-14 3.1.0(`f96d785`) 종합 검토 보고서(저장소에 두지 않음)의 새 발견을 코드로 확인해 옮겼다. 그 보고서의 기존 항목 재평가는 B3-38·B3-39·B3-52 행에 합쳤다(설정 저장 실패 안내는 3.4.2에서 고쳤다).

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-114 | 사용자 강조색에 따라 포커스 선·버튼 글자의 대비가 무너짐 | `ink_on`은 패널 휘도가 0.2 미만이면 대비를 보지 않고 그만두고, `accent_text`는 선형화하지 않은 RGB 가중합의 임계값(0.52, 2.2.13)으로 글자 색을 고른다. `theme.rs` 계산을 Python으로 옮겨 잰 값: 어두운 패널과 같은 `#24262B`이면 선/패널 1.000:1(포커스 선이 보이지 않음), `#808080`이면 글자/강조 3.786:1, 기본 `#FFC94A`는 어두운 테마 9.880:1·글자 11.572:1, 밝은 테마 선 3.036:1. 앱 화면으로 본 값은 아니다. 고치려면 고른 색은 두고 배경마다 밝게·어둡게 파생하고, 글자는 두 후보의 실제 대비로 고른다. 닫는 조건: 검정·흰색·중간 회색·패널과 같은 색·고채도 색을 두 테마에서 글자 4.5:1, 선 3:1로 시험 | `apps/ugurugu/src/theme.rs` `ink_on`·`accent_text`, `widgets.rs` `focus_ring` | 3.1.0 종합 검토 N01 |
| B3-117 | Cargo 의존성 갱신 주기 | Dependabot은 GitHub Actions만 매달 본다. Cargo 의존성은 정해진 검토 주기가 없다. 자동 major 갱신보다 호환 묶음을 native 목록·회귀 화소·성능 기준선과 함께 확인하는 주기를 정한다. B3-44·B3-45·B3-72를 그때 같이 본다 | `.github/dependabot.yml`, `Cargo.toml` | 3.1.0 종합 검토 9.3절 |
| B3-118 | `files.rs`의 상태 전이 나누기 | `files.rs`(3,371행, 2026-10-16)에 파일 작업자, 저장 상태, 복구, 대화상자, 내보내기 질문이 모인다. 파일만 쪼개면 같은 가변 상태를 공유해 이해 비용이 줄지 않으므로, "질문 대기 → 특정 탭의 편집 확정 → 저장 요청 → 특정 상태의 응답 → 닫기·업데이트" 전이를 먼저 그리고 대화상자 표시·작업자 메시지·탭의 저장·복구 상태를 나눈다. 탭 id·저장 당시 상태·요청 id가 만나는 곳에는 타입 구분을 더한다. 닫는 조건: 늦은 저장 응답, 질문 중 탭 전환, 닫기 취소, 같은 경로로 저장, 내보내기 중 업데이트가 전과 같이 동작 | `apps/ugurugu/src/files.rs`, `canvas.rs` | 검토 4절, 3.1.0 종합 검토 6절 |

## 12. 3.2.0 애니메이션에서 뒤로 미룬 것

계획(`docs/rust/plan-animation.md`, 지운 뒤에는 커밋 `d0506ba`에 있음) 9절과 구현하며 남긴 것. 정한 것은 decisions.md 4절 "키프레임과 애니메이션 폴더"와 ADR 8절에 있다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-120 | 캔버스 밖 여백 | 레이어 표면이 캔버스 크기라 레이어를 옮기면 가장자리가 비어 들어온다. 캔버스 밖에서 들어오는 움직임에는 표면에 여백이 있어야 하고, 타일 계산·예산이 바뀐다 | `crates/ugu-render/src/document.rs` `reach` | 계획 4.5 |
| B3-133 | 프레임마다 움직이는 레이어의 병합 | 한 자리에 둔 레이어는 이동을 구워 병합한다(decisions.md 4절). 자세가 바뀌는 레이어는 구간이 자기 키를 가져야 같은 모습이 되어 형식이 바뀐다. 키가 같은 두 레이어라도 각각 옮겨 합성한 것과 합성해 옮긴 것은 가장자리 화소가 다르고, 불투명도 키는 레이어마다 따로 곱한다 | `crates/ugu-core/src/ops.rs` `merge_down` | 3.2.0 계획 9절 |
| B3-134 | 곡선 경로·베지어 보간 편집 | 키 사이는 일정·부드럽게·멈춤만 있고 이동은 직선이다. 경로를 곡선으로 그리고 손잡이로 보간 곡선을 고치는 UI와 형식이 필요하다 | `crates/ugu-core/src/keys.rs` `pose` | 3.2.0 계획 9절 |
| B3-135 | 셀 복사로 노출 채우기 | 셀을 복사해 여러 프레임에 노출하는 편집(예: 같은 셀을 다음 빈 프레임들에 잇기). 무엇을 채울지(빈 프레임만, 범위) 정해야 한다 | `crates/ugu-session/src/animating.rs` | 3.2.0 계획 9절 |
| B3-124 | 움직인 레이어의 브러시 커서 크기와 큰 선택 끌기 | 브러시 커서는 레이어 크기 키를 곱하지 않고 문서 크기로 보인다. 움직인 레이어에서 큰 선택을 끄는 동안은 화면 해상도 미리보기(`Glance`) 대신 원본으로 그린다 | `apps/ugurugu/src/canvas/cursor.rs`, `canvas/transform.rs` `refresh_preview` | 3.2.0 구현 |
| B3-125 | 움직인 레이어에서 그릴 때의 입력→표시 | headless pen-up만 쟀다(evidence.md 55절). Sandbox는 합성 입력이라 앱의 입력→표시 지연(latency-probe)은 사용자 PC에서 잰다 | `tools/latency-probe` | 계획 7절 |
| B3-127 | 키가 있는 레이어의 이미지 크기 바꾸기 | `resample_image`는 키의 이동값과 중심을 축마다 비율로 늘린다. 가로세로 비율이 다르게 바뀌면 회전한 키는 늘어난 화소와 어긋나고(자세는 한 가지 크기와 회전만 가짐), 늘린 값이 `limits::OFFSET`(32,767)을 넘으면 문서 검사에 걸려 크기 바꾸기 전체가 거절된다. 2026-10-14 재검토에서 코드로 확인했고 앱에서 재현하지는 않았다 | `crates/ugu-core/src/command.rs` `resample_image`, `rekeyed` | 3.2.0 재검토 |

## 13. 3.3.0 출시 전 재검토에서 남은 것

2026-10-15 3.2.1 이후 작업(#76~#80, 캔버스 크기 테두리)의 재검토에서 확인했고 이번에 고치지 않은 것.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-140 | 대칭자의 남은 것 | 대칭자(decisions.md 4절)는 파일에 저장하지 않고, 도형 도구는 따르지 않는다. 후보: 파일 저장(형식에 선택 필드), 도형과 채운 도형의 복사, 복사가 같이 우글거리는 선택지. 큰 획이 많으면 펜을 뗄 때가 길다: 2048²에서 문서 대부분을 덮는 획 24개는 약 40ms이고, 그 절반 가까이가 획 경계 사각형마다 coverage를 읽어 놓는 일이다(evidence.md 68절) | `crates/ugu-session/src/symmetry.rs`, `apps/ugurugu/src/canvas/symmetry.rs`, `crates/ugu-render/src/compose.rs` `stamp_strokes` | 2026-10-15 대칭자 |

## 14. 3.3.1 출시에서 남은 것

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-141 | 막 뜬 창이 닫기 요청에 30초 안에 끝나지 않음 | 3.3.1 수용 시험의 "Run"이 설치 뒤 첫 실행에서 `CloseMainWindow` 뒤 30초 안에 끝나지 않았다(evidence.md 70절). 로그는 업데이트 확인 실패(오프라인)에서 멈췄고 닫기는 로그에 남지 않는다. 같은 빌드로 다시 돌린 수용 10/10에서는 나오지 않았다. 시작 직후 창 스레드가 메시지를 돌리지 않는 B3-97과 같은 원인일 수 있다(가설). 2026-10-17부터 닫기 경로가 로그에 남는다: 창 스레드가 받음(`close requested`, 창이 준비되기 전이면 `not taken` 경고), 기다림(`closing waits`와 저장·대화상자·열기 수)·내보내기 질문·저장 안 한 변경 질문, render 스레드 끝(`the render thread ends`), 창 닫힘(`the window closes`). 다음에 나오면 어느 줄까지 남았는지로 창 스레드가 메시지를 받지 못했는지, 질문이나 기다림에 걸렸는지 가른다 | `apps/ugurugu/src/app.rs` 시작·닫기 순서 | evidence.md 70절 |

## 15. 2026-10-21 UI/UX 검토에서 남은 것

헤드리스로 그린 화면 65장과 UI 코드로 본 검토(evidence.md 88절). 선택 카드 설명 잘림, 아이콘 재사용, 영어로 고정된 새 레이어 이름, 기본값 복원 확인, 우글거림 애니메이션 설명, 문구 통일은 PR #117에서 고쳤다(decisions.md 9절). 아래는 남은 것이다.

| ID | 항목 | 왜·영향 | 어디 | 출처 |
|---|---|---|---|---|
| B3-156 | 캔버스 배경(투명 포함)을 정할 수 없음 | 2.2.13은 편집 > "캔버스 배경…"(알파를 고르는 색 대화상자, `git show v2.2.13:src/ui/MainWindowActions.cpp` 306·1003행)이 있었는데 3.0에서 결정 기록 없이 빠졌다. `Document.background`와 `Op::SetSettings`는 있다. 그래서 3.x에서 만든 그림은 늘 흰 배경이고, GIF·WebP 창의 "투명한 부분 유지"가 늘 꺼진 채 이유만 말한다(2.x에서 가져온 투명 배경 문서만 켜짐). 되살리면 편집 메뉴 항목, 알파를 고르는 색 선택, 실행 취소 한 단계, 그리고 내보내기 안내가 그 메뉴를 가리키게 한다 | `apps/ugurugu/src/ui/actions.rs` `EDIT`, `crates/ugu-core/src/edit.rs` `SetSettings`, `strings.txt` `export-opaque-background` | 2026-10-21 UI 검토 |
| B3-157 | 레이어 동작과 브러시 크기에 단축키·메뉴가 없음 | 동작 표(74개)에 레이어 동작이 없어 새 레이어·복제·아래와 병합·삭제·숨기기·위아래 이동은 레이어 패널 버튼으로만 된다(레이어 메뉴도 없다). 브러시 크기 `[`·`]`와 현재·이전 색 바꾸기 `X`도 없다. 기본 키를 정할 때 지금의 Ctrl+E(GIF 내보내기)·Ctrl+Shift+N(새 셀)과 포토샵·클립 스튜디오 관례(아래와 병합 Ctrl+E, 새 레이어 Ctrl+Shift+N)를 맞춰 봐야 한다. 새 기본 키는 전 판에서 고른 키를 가져가지 않는다(decisions.md 9절) | `apps/ugurugu/src/shortcuts.rs` `actions!`, `ui/actions.rs`, `ui/layers.rs` | 2026-10-21 UI 검토 |
| B3-158 | 메뉴 구성 | 편집 메뉴가 20항목이다(2.2.13은 선택 하위 메뉴). 보기 메뉴는 애니메이션 도구를 켜면 19항목이고, 그중 키 추가/삭제·애니메이션 폴더 만들기·새 셀은 문서를 바꾸는 편집이다. 선택·레이어(B3-157)·애니메이션 메뉴로 나누는 안 | `apps/ugurugu/src/ui/actions.rs` `MENUS` | 2026-10-21 UI 검토 |
| B3-159 | 강조색이 많은 곳에 칠해짐 | 모든 슬라이더의 채운 부분(`theme.rs`의 `slider_trailing_fill`과 `selection.bg_fill`)과 레이어 줄마다의 우글거림 아이콘이 앰버라 decisions.md 9절의 "강조색 채움은 지금 켜진 것 하나" 원칙과 어긋나고 현재 도구 표시가 약해진다. 경고·거절 안내도 같은 앰버로 상태 표시줄 왼쪽 아래에 나와(`ui/mod.rs` `status_bar`) 펜 근처에서 거절당한 사용자가 놓치기 쉽다(가설, 사용자 시험 없음) | `apps/ugurugu/src/theme.rs`, `ui/layers.rs`, `ui/mod.rs` | 2026-10-21 UI 검토 |
| B3-160 | 새 문서 크기 버튼이 픽셀 그림용뿐 | 16·32·64·128만 있다(`files.rs` `ask_new_document`). 512², 1080², 1920×1080처럼 움직이는 그림에 흔한 크기가 없다 | `apps/ugurugu/src/files.rs` | 2026-10-21 UI 검토 |
| B3-161 | 작은 안내·표시 | 저장 확인(`unsaved-text`)이 어느 문서인지 이름을 말하지 않는다(창 닫기는 탭을 차례로 보여 주며 묻는다). 단축키 페이지가 74개를 메뉴별 묶음 없이 한 목록(줄 약 52pt)으로 보인다. 도구 설정 머리가 늘 "도구 설정"이라 지금 도구 이름이 없다(도구 막대는 아이콘만). 색 칸의 `#AARRGGBB`(2.2.13)는 웹·디자인 도구가 `#RRGGBBAA`로 읽어 복사하면 다른 색이 된다 | `apps/ugurugu/i18n/strings.txt`, `ui/settings.rs`, `ui/dock.rs`, `ui/color.rs` | 2026-10-21 UI 검토 |
| B3-162 | 정밀 터치패드의 두 손가락 스크롤 | 캔버스 위 세로 휠은 늘 확대·축소이고 가로 휠은 캔버스가 받지 않는다(`input.rs` `PointerEvent::Wheel`). 그래서 터치패드의 두 손가락 끌기가 이동이 아니라 확대·축소가 될 것이다(가설, 장치 없음) | `apps/ugurugu/src/input.rs` | 2026-10-21 UI 검토 |
| B3-164 | 작은 창 배치(#121·#122)를 Sandbox에서 확인 | #121·#122는 헤드리스 시험과 AccessKit 측정(evidence.md 91절)으로만 확인했다. 사용자가 GUI 실행을 미뤄 Sandbox로는 보지 않았다. 볼 것: 150%의 물리 1600×1000 창과 새 설정의 첫 실행 최대화, 실제 포인터로 패널 사이 경계를 끌 때의 높이와 커서, 포커스 뒤 ↑↓, 목록이 열린 동안 ↓가 목록 안에 머무는지, 메뉴 줄 오른쪽의 패널·실행 취소·다시 실행, 우글거림 행 숨김. `gui.ps1`·`pointer.ps1`은 창 크기를 정하기 전에 창을 복원하게 바뀌었으니(새 설정이면 최대화로 열림) 기존 단계가 그대로 통과하는지도 본다. 다음 출시 전 GUI 실행에서 같이 한다 | `tools/sandbox/gui.ps1`, `pointer.ps1`, `apps/ugurugu/src/ui/dock.rs` | 2026-10-21 |
