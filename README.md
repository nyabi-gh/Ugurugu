<p align="center">
  <img src="resources/icons/Ugurugu.png" width="112" alt="Ugurugu 앱 아이콘">
</p>

<h1 align="center">Ugurugu</h1>

<p align="center">
  그림이 우글우글 움직이는 드로잉 앱입니다.
</p>

<p align="center">
  <a href="https://github.com/nyabi-gh/Ugurugu/releases/latest"><img src="https://img.shields.io/github/v/release/nyabi-gh/Ugurugu?style=flat-square&color=ffc94a" alt="Latest release"></a>
  <a href="https://github.com/nyabi-gh/Ugurugu/releases"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyabi-gh%2FUgurugu%2Fdownload-badge%2Fdownloads.json&style=flat-square" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-ffc94a?style=flat-square" alt="License"></a>
</p>

<p align="center"><b>KR</b> · <a href="README.en.md">EN</a> · <a href="README.ja.md">JP</a></p>

> [!IMPORTANT]
> **Ugurugu 3.0은 새로 만든 Windows 앱입니다.**
> - macOS는 아직 2.2.13을 쓰세요. macOS용 3.x는 나중에 지원합니다.
> - 3.0은 2.x와 따로 설치되며, 둘을 함께 둘 수 있습니다. 2.x는 3.0으로 자동 업데이트되지 않습니다.
> - 작업 파일은 새 형식 `.ugurugu`입니다. 2.1.0 이후에 저장한 `.ugu`는 3.0에서 열어 새 형식으로 저장할 수 있습니다. 3.0의 `.ugurugu`는 2.x에서 열 수 없습니다.

그린 선이 저절로 우글우글 흔들리는 애니메이션이 됩니다. 완성한 그림은 반복
재생되는 GIF·WebP나 PNG·JPEG 이미지로 저장할 수 있습니다.

## 다운로드

| 플랫폼 | 지원 환경 | 다운로드 |
| --- | --- | --- |
| Windows | Windows 10 이상, 64비트 | [Setup.exe](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-Windows-x64-Setup.exe) |
| macOS (2.2.13) | macOS 14 이상, Apple Silicon | [DMG](https://github.com/nyabi-gh/Ugurugu/releases/download/v2.2.13/Ugurugu-macOS-arm64.dmg) |

- **Windows**: Setup 파일을 실행하면 관리자 권한 없이 사용자 계정에 설치됩니다.
  코드 서명이 없어 Windows가 "PC 보호" 경고를 띄울 수 있습니다.
  **추가 정보 → 실행**을 누르면 됩니다. 받은 파일은 릴리스 페이지의
  `SHA256SUMS-Windows.txt`로 확인할 수 있습니다.
- **macOS**: DMG를 열고 Ugurugu를 Applications 폴더로 드래그하세요.

반드시 공식 [Releases 페이지](https://github.com/nyabi-gh/Ugurugu/releases/latest)에서
받은 파일만 사용하세요. 릴리스 페이지의 다른 파일은 자동 업데이트용이므로
받지 않아도 됩니다.

<details>
<summary>시스템 사양 (Windows)</summary>

| 항목 | 내용 |
| --- | --- |
| 운영체제 | Windows 10 이상 64비트. Windows 11에서 시험했습니다 |
| 그래픽 | Direct3D 12를 지원하는 GPU. 쓸 수 없으면 소프트웨어 렌더링(WARP)으로 자동 전환되며, 이때는 조금 느립니다 |
| 메모리 | 큰 캔버스(최대 4096×4096)에 레이어를 여러 장 두면 문서 하나에 수백 MB를 씁니다. 탭을 여러 개 열면 그만큼 더 씁니다 |
| 입력 장치 | 마우스, 필압 펜(Windows Ink), 두 손가락 터치 |

</details>

## 처음 시작하기

1. **파일 → 새로 만들기**로 캔버스를 만들거나 기존 그림을 엽니다. 문서마다 캔버스 위에 탭이 생깁니다.
2. 왼쪽에서 브러시를 고르고 선을 그립니다.
3. **우글거림** 패널에서 움직임을 고른 뒤 `P`를 눌러 재생합니다.
4. **파일** 메뉴에서 GIF·WebP 또는 PNG·JPEG로 내보냅니다.

막히면 `F1`을 눌러 앱 안의 도움말을 보세요.

## 주요 기능

- **우글거림** — 클래식·부드럽게·단계별 세 가지 움직임과 흔들림·포즈 수·
  디테일·연결·무작위성 조절, 끊어진 선 효과. 그림 전체에 걸거나 레이어마다
  따로 정할 수 있어 배경은 멈추고 선만 움직이게 할 수도 있습니다.
- **그리기** — 펜·마커·에어브러시·스프레이·픽셀 18가지 브러시와 지우개 4가지,
  크기·불투명도·손떨림 보정, 필압, 페인트 통, 자유형·사각형·타원 선택과
  자동 선택, 선택 영역 변형, 글자 도구, 선택한 선의 색·굵기 바꾸기.
- **픽셀 모드** — 픽셀 그림용 문서 모드. 픽셀 브러시만 쓰고, 경계를 섞지
  않으며, 변형·크기 변경·우글거림이 픽셀 단위로 움직이고, 확대하면 픽셀
  격자가 보입니다.
- **레이어** — 그룹, 불투명도, 합성 모드, 아래 레이어에 클리핑, 참조 레이어,
  이미지 넣기, 캔버스 크기·이미지 크기 변경.
- **문서 탭** — 한 창에서 여러 그림을 탭으로 오갑니다(`Ctrl+Tab`). 파일
  탐색기에서 연 그림도 이미 떠 있는 창의 새 탭으로 열립니다.
- **저장과 내보내기** — 작업 파일은 `.ugurugu`. GIF·WebP 애니메이션, PNG·JPEG
  현재 프레임. 도구 설정은 `.ugurugu-tools`로 주고받을 수 있고, 갑자기 꺼져도
  다음 실행 때 탭마다 작업을 되살립니다.
- **사용자 설정** — 패널을 끌어다 쌓기·나란히 붙이기·탭으로 묶기·띄우기,
  어두운·밝은 테마와 강조 색, 색상 패널의 사각형·삼각형, 모든 단축키 변경,
  한국어·영어·일본어 화면, 앱 안에서 업데이트.

설정은 **편집 → 설정**(`Ctrl+,`)에서 엽니다. 패널 배치는 **창 → 패널 배치
초기화**로 되돌릴 수 있습니다. 새 버전은 실행할 때 확인하며(설정에서 끌 수
있음), **도움말 → 업데이트 확인**으로 직접 확인할 수도 있습니다.

<details>
<summary>우글거림 설정 자세히 보기</summary>

패널 위쪽의 범위를 **활성 레이어**로 바꾸면 선택한 레이어에만 적용됩니다.

| 설정 | 범위 (기본값) | 설명 |
| --- | --- | --- |
| 모션 스타일 | 클래식 · 부드럽게 · 단계별 (클래식) | **클래식**은 프레임마다 새로 흔들리는 이전 버전의 움직임, **부드럽게**는 포즈 사이를 흐르듯 잇는 움직임, **단계별**은 포즈를 툭툭 갈아 끼우는 손그림 애니메이션 느낌입니다. |
| 흔들림 | 0 ~ 12 px (1.6 px) | 선이 원래 자리에서 벗어나는 최대 거리. 0이면 움직이지 않습니다. |
| 포즈 수 | 1 ~ 프레임 수 (8) | 반복해 쓰는 서로 다른 그림의 개수. 적을수록 뚝뚝 끊기고 많을수록 매끄럽습니다. 1이면 멈춥니다. |
| 디테일 | 1 ~ 24 (12) | 선을 따라 생기는 흔들림의 간격. 낮으면 완만한 물결, 높으면 잘게 떠는 느낌입니다. |
| 연결 | 0 ~ 100% (100%) | 선들이 한 몸처럼 움직이는 정도. 100%면 그림 전체가 함께, 0%면 선마다 제각각 움직입니다. |
| 무작위성 | 0 ~ 100% (0%) | 높일수록 점마다 튀는 지글지글한 노이즈가 됩니다. |
| 끊어진 선 | 켬 · 끔 (끔) | 선의 일부가 사라졌다 나타납니다. 아래 두 값은 이걸 켜야 적용됩니다. |
| 끊김 정도 | 0 ~ 100% (35%) | 선이 사라지는 양. 100%면 통째로 사라집니다. |
| 끊김 범위 | 2 ~ 256 px (24 px) | 끊기는 덩어리의 길이. 작으면 점선처럼, 크면 뭉텅뭉텅 끊깁니다. |

**포즈 수·디테일·연결·무작위성**은 *부드럽게*·*단계별*에서만 적용되고,
*클래식*은 사용하지 않습니다. 애니메이션은 2 ~ 60 프레임, 1 ~ 50 fps입니다.

</details>

<details>
<summary>내보내기 자세히 보기</summary>

| 형식 | 내용 |
| --- | --- |
| GIF | 반복 재생. 색은 애니메이션 전체에서 팔레트 하나를 씁니다. 투명은 켜고 끄기만 있어, 반투명한 부분은 투명하거나 불투명하게 나뉩니다 |
| WebP | 반복 재생, 무손실, 반투명 그대로 |
| PNG | 현재 프레임, 투명 그대로 |
| JPEG | 현재 프레임, 투명한 부분은 흰색 |

애니메이션은 문서 크기, 더 작은 크기, 또는 픽셀을 그대로 키운 2·4·8배로
내보낼 수 있어 픽셀 그림도 선명하게 키울 수 있습니다. 배경이 투명한 문서는 **투명 부분 유지**를 고를 수 있고,
끄면 흰 바탕에 내보냅니다.

</details>

## 이전 버전 파일

- **2.1.0 이후에 저장한 `.ugu`**: **파일 → 열기**로 엽니다. 원본은 그대로 두고 새 작업으로
  열리며, 처음 저장할 때 `.ugurugu`로 저장합니다. 3.0으로 옮기지 못하는 내용이
  있으면 앱이 알려 줍니다.
- **2.1.0보다 오래된 `.ugu`, `.wagle`·`.wobble`, WiggleWiggleTool의 `.wawa`**:
  3.0에서는 바로 열 수 없습니다. 2.2.13에서 열어 `.ugu`로 저장한 뒤 3.0에서
  여세요.

## 단축키

자주 쓰는 기본값입니다. **설정 → 단축키**에서 모두 바꿀 수 있습니다.

| 키 | 동작 |
| --- | --- |
| `B` / `E` | 브러시 / 지우개 |
| `L` / `W` / `G` / `T` | 영역 선택 / 자동 선택 / 페인트 통 / 글자 |
| `I` 또는 `Alt` + 클릭 | 색상 가져오기 |
| `P` | 재생 / 일시정지 |
| `Space` + 드래그, 스크롤 | 캔버스 이동, 확대·축소 |
| `Ctrl+Z` / `Ctrl+Y` | 실행 취소 / 다시 실행 |
| `Ctrl+Tab` / `Ctrl+W` | 다음 탭 / 탭 닫기 |
| `F1` | 도움말 |

<details>
<summary>전체 단축키</summary>

| 키 | 동작 |
| --- | --- |
| 두 손가락 드래그·핀치·비틀기 | 캔버스 이동·확대/축소·회전 (터치 화면) |
| 가운데 버튼 드래그 | 캔버스 이동 |
| `Ctrl+Space` + 드래그 | 캔버스 확대·축소 |
| `Shift+Space` + 드래그 | 캔버스 자유 회전 |
| `Shift` + 스크롤 | 캔버스를 5°씩 회전 |
| `-` / `=` | 캔버스를 왼쪽/오른쪽으로 5° 회전 |
| `M` | 캔버스 좌우 반전 |
| `Ctrl+0` / `Ctrl+1` | 캔버스를 창에 맞추기 / 실제 픽셀 크기 |
| `Ctrl++` / `Ctrl+-` | 확대 / 축소 |
| `Ctrl+N` / `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` | 새로 만들기 / 열기 / 저장 / 다른 이름으로 저장 |
| `Ctrl+E` / `Ctrl+Shift+E` | GIF 내보내기 / 현재 프레임 이미지로 내보내기 |
| `Ctrl+Shift+Tab` | 이전 탭 |
| `Ctrl+Shift+Z` | 다시 실행 |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | 복사 / 잘라내기 / 붙여넣기 |
| `Ctrl+A` / `Ctrl+D` / `Ctrl+Shift+I` | 모두 선택 / 선택 해제 / 선택 반전 |
| `Ctrl+T` | 선택 영역 변형 |
| `Alt+Delete` | 선택 영역을 브러시 색으로 채우기 |
| `Delete` | 선택한 내용 지우기 |
| `Enter` / `Esc` | 변형·글자 적용 / 취소 |
| `Ctrl+Shift+T` | 아래쪽 애니메이션 바 접기 / 펴기 |
| `Ctrl+,` | 설정 |
| `Alt` 또는 `F10` | 메뉴로 이동 |

</details>

## 문제 신고

[GitHub 이슈](https://github.com/nyabi-gh/Ugurugu/issues)에 어떤 작업을 했는지와
실제로 일어난 일을 적어 주세요. **설정 → 정보**에서 진단 정보를 복사하고 로그
폴더를 열 수 있으니 함께 붙여 주시면 큰 도움이 됩니다. 가능하면 `.ugurugu`
파일도 함께 보내 주세요. 보안 취약점은 공개 이슈 대신 [보안 정책](SECURITY.md)의
비공개 경로로 알려 주세요.

## 개발과 기여

소스 빌드는 [BUILDING.md](BUILDING.md), 기여 방법은 [CONTRIBUTING.md](CONTRIBUTING.md)를
참고하세요.

## 크레딧과 라이선스

- Development support by seuppi
- App icon artwork by seuppi (`resources/icons/`, GPL-3.0-or-later로 배포)

Copyright (C) 2026 Nyabi (nyabi-gh)

이 프로그램은 자유 소프트웨어로, [GNU General Public License](LICENSE)
버전 3 또는 (선택에 따라) 그 이후 버전의 조건에 따라 재배포하거나 수정할 수
있습니다. 어떠한 보증도 제공하지 않습니다. 기여물도 같은 조건으로 받으며,
포함된 글꼴과 라이브러리의 조건은 [서드파티 고지](THIRD_PARTY_NOTICES.md)에
있습니다.

SPDX: `GPL-3.0-or-later`
