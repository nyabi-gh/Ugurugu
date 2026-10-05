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

완성한 그림은 반복 재생되는 GIF·WebP나 배경이 투명한 이미지로 저장할 수
있고, WiggleWiggleTool의 `.wawa` 그림도 이어서 작업할 수 있습니다.

## 다운로드

| 플랫폼 | 지원 환경 | 다운로드 |
| --- | --- | --- |
| Windows | Windows 10 이상, 64비트 | [Setup.exe](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-Windows-x64-Setup.exe) |
| macOS | macOS 14 이상, Apple Silicon | [DMG](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-macOS-arm64.dmg) |

- **Windows**: Setup 파일을 실행하세요. 출처 확인 경고가 나오면
  **추가 정보 → 실행**을 누르면 됩니다.
- **macOS**: DMG를 열고 Ugurugu를 Applications 폴더로 드래그하세요.
  Apple의 확인을 거쳐 배포됩니다.

반드시 공식 [Releases 페이지](https://github.com/nyabi-gh/Ugurugu/releases/latest)에서
받은 파일만 사용하세요. 릴리즈 페이지의 다른 파일은 자동 업데이트용이므로
받지 않아도 됩니다.

<details>
<summary>시스템 사양</summary>

| 항목 | 최소 | 권장 |
| --- | --- | --- |
| 운영체제 | Windows 10 64비트 / macOS 14 (Apple Silicon) | Windows 11 / 최신 macOS |
| 메모리 | 8GB | 16GB 이상 |
| 그래픽 | 별도 요구 없음 | Direct3D 11(Windows) 또는 Metal(macOS) 지원 GPU |
| 입력 장치 | 마우스 | 필압을 지원하는 펜 태블릿 (Wacom 등) |

캔버스 표시와 확대·이동·재생은 GPU로 처리하며, 그래픽 가속을 쓸 수 없으면
소프트웨어 렌더링으로 자동 전환됩니다. 큰 캔버스(최대 4096×4096)에 레이어를
여러 장 둔다면 메모리가 넉넉할수록 미리보기가 매끄럽습니다.

</details>

## 처음 시작하기

1. 새 캔버스를 만들거나 기존 그림을 엽니다.
2. 왼쪽에서 브러시를 고르고 선을 그립니다.
3. **우글거림** 패널에서 움직임을 고른 뒤 `P`를 눌러 재생합니다.
4. **파일** 메뉴에서 GIF·WebP 또는 PNG·JPG로 내보냅니다.

막히면 `F1`을 눌러 앱 안의 도움말을 보세요.

## 주요 기능

- **우글거림** — 클래식·부드럽게·단계별 세 가지 움직임과 흔들림·디테일·
  연결·무작위성 조절, 끊어진 선 효과. 레이어마다 켜고 끌 수 있어 배경은
  멈추고 선만 움직이게 할 수도 있습니다.
- **그리기** — 필압 브러시·지우개를 포함한 17가지 기본 도구, 손떨림 보정,
  페인트 통, 자유형·사각형·타원 선택과 변형, 멀티터치 화면에서 두 손가락으로
  이동·확대·회전.
- **레이어** — 그룹, 불투명도, 합성 모드, 이미지 가져오기, 캔버스 자르기·
  넓히기·크기 변경.
- **저장과 내보내기** — 작업 파일은 `.ugu`(이전 버전의 `.wagle`·`.wobble`도
  열림). 투명 배경 GIF·WebP·PNG·JPG로 내보내기, `.wwpreset`으로 설정 공유,
  비정상 종료 시 작업 복구.
- **사용자 설정** — 패널을 끌어다 쌓기·나란히 붙이기·탭으로 묶기, 강조 색상,
  모든 단축키 변경, 한국어·영어·일본어 화면, 앱 안에서 업데이트.

설정은 툴바의 톱니바퀴 버튼(또는 Windows **편집 → 설정**, macOS
**Ugurugu → 설정**)에서 열 수 있습니다. 패널 배치는 **창 → 패널 배치 초기화**로
되돌릴 수 있고, 새 버전은 실행할 때 자동으로 확인하며 **도움말 → 업데이트
확인**으로 직접 확인할 수도 있습니다.

<details>
<summary>우글거림 설정 자세히 보기</summary>

패널 위쪽의 범위를 **활성 레이어**로 바꾸면 선택한 레이어에만 적용됩니다.

| 설정 | 범위 (기본값) | 설명 |
| --- | --- | --- |
| 모션 스타일 | 클래식 · 부드럽게 · 단계별 (클래식) | **클래식**은 이전 버전과 같은 움직임, **부드럽게**는 포즈 사이를 흐르듯 잇는 움직임, **단계별**은 포즈를 툭툭 갈아 끼우는 손그림 애니메이션 느낌입니다. |
| 흔들림 | 0 ~ 12 px (1.6 px) | 선이 원래 자리에서 벗어나는 최대 거리. 0이면 움직이지 않습니다. |
| 포즈 수 | 1 ~ 프레임 수 (8) | 반복해 쓰는 서로 다른 그림의 개수. 적을수록 뚝뚝 끊기고 많을수록 매끄럽습니다. 1이면 멈춥니다. |
| 디테일 | 1 ~ 24 (12) | 선을 따라 생기는 흔들림의 간격. 낮으면 완만한 물결, 높으면 잘게 떠는 느낌입니다. |
| 연결 | 0 ~ 100% (100%) | 선들이 한 몸처럼 움직이는 정도. 100%면 그림 전체가 함께, 0%면 선마다 제각각 움직입니다. |
| 무작위성 | 0 ~ 100% (0%) | 높일수록 점마다 튀는 지글지글한 노이즈가 됩니다. |
| 끊어진 선 | 켬 · 끔 (끔) | 선의 일부가 사라졌다 나타납니다. 아래 두 값은 이걸 켜야 적용됩니다. |
| 끊김 정도 | 0 ~ 100% (35%) | 선이 사라지는 양. 100%면 통째로 사라집니다. |
| 끊김 범위 | 2 ~ 256 px (24 px) | 끊기는 덩어리의 크기. 작으면 점선처럼, 크면 뭉텅뭉텅 끊깁니다. |

**포즈 수**와 **디테일**은 *부드럽게*·*단계별*에서만 적용되고, *클래식*은
사용하지 않습니다.

</details>

## WiggleWiggleTool 그림 이어하기

WiggleWiggleTool 10의 `.wawa` 파일을 **파일 → 열기**로 불러올 수 있습니다.
원본은 그대로 두고 새 작업으로 열리며, 처음 저장할 때 같은 이름의 `.ugu`를
제안합니다. 두 앱의 표현 방식이 달라 일부 우글거림·에어브러시·채워진 모양은
조금 다르게 보일 수 있고, 바뀌거나 가져오지 못한 항목은 앱이 알려 줍니다.

## 단축키

자주 쓰는 기본값입니다. **설정 → 단축키**에서 모두 바꿀 수 있습니다.

| 키 | 동작 |
| --- | --- |
| `B` / `E` | 브러시 / 지우개 |
| `L` / `W` / `G` | 영역 선택 / 자동 선택 / 페인트 통 |
| `I` 또는 `Alt` + 클릭 | 색상 가져오기 |
| `P` | 재생 / 일시정지 |
| `Space` + 드래그, 스크롤 | 캔버스 이동, 확대·축소 |
| `Ctrl/Cmd+Z` | 실행 취소 |
| `F1` | 도움말 |

<details>
<summary>전체 단축키</summary>

| 키 | 동작 |
| --- | --- |
| 두 손가락 드래그·핀치·비틀기 | 캔버스 이동·확대/축소·회전 (지원되는 멀티터치 화면) |
| `Shift+Space` + 드래그 | 캔버스 자유 회전 |
| `Shift` + 스크롤 | 캔버스를 5°씩 회전 |
| `-` / `^` | 캔버스를 왼쪽/오른쪽으로 5° 회전 |
| **보기 → 캔버스 회전 → 캔버스 회전 초기화** | 회전을 0°로 초기화 |
| `Alt+Delete` | 선택 영역을 브러시 색으로 채우기 |
| `Ctrl+T` | 아래쪽 애니메이션 바 접기 / 펴기 |
| `Ctrl/Cmd+C`, `X`, `V` | 복사 / 잘라내기 / 붙여넣기 |
| `Ctrl+Y` (Windows), `Cmd+Shift+Z` (macOS) | 다시 실행 |
| `Ctrl/Cmd+0` | 캔버스를 창에 맞추기 |
| `Ctrl/Cmd+1` | 실제 픽셀 크기로 보기 |
| `Enter` / `Esc` | 변경 적용 / 취소 |

</details>

## 문제 신고

[GitHub 이슈](https://github.com/nyabi-gh/Ugurugu/issues)에 어떤 작업을 했는지와
실제로 일어난 일을 적어 주세요. 가능하면 `.ugu` 파일도 함께 보내 주시면 큰
도움이 됩니다. 보안 취약점은 공개 이슈 대신 [보안 정책](SECURITY.md)의 비공개
경로로 알려 주세요.

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
