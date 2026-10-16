# 독립 실행 패키지

[English](packaging.md) | [한국어](packaging_KR.md)

Rust 데스크톱·CLI·내부 파서 프로세스와 LibRaw·TagLib·검토한 FFmpeg 공유 라이브러리
5개를 함께 제공합니다. 나머지 추출 라이브러리는 실행 파일에 연결합니다. 실행할 때
Python·Rust·CMake·ExifTool·ffprobe나 시스템에 별도로 설치한 메타데이터 라이브러리가
필요하지 않습니다. 압축을 푼 폴더 전체를 함께 유지하세요.

## 실행

| 플랫폼 | 데스크톱 | CLI |
|---|---|---|
| Windows x64 | `metaxtract-gui.exe` | `metaxtract.exe --help` |
| Linux x64 | `./metaxtract-gui` | `./metaxtract --help` |
| macOS, 패키지의 아키텍처 | `MetaXtract.app` | `MetaXtract.app/Contents/MacOS/metaxtract --help` |

새 사용자 소유 폴더에 압축을 푼 뒤 데스크톱을 실행하세요. Windows 10 이상용 패키지는 MSVC의
앱 로컬 릴리스 런타임도 포함합니다. Linux는 운영체제의 glibc·C++ 런타임과 데스크톱
그래픽 기능을 사용합니다. `package.json`에 필요한 최소 glibc와 연결한 OS 라이브러리를
기록하고 C++ ABI 요구 사항도 포함합니다. 이전 배포판이나 다른 CPU에서의 호환성을
보장하지는 않습니다. macOS 번들도 필요한 최소 OS 버전을 기록합니다. Linux에는
정상적인 X11·Wayland 그래픽 환경과 한국어 표시용 글꼴이 필요합니다. macOS는 시스템
프레임워크를 사용합니다. 현재 macOS CI 패키지는 임시 서명을 사용하며 Developer ID
공증은 하지 않습니다. Gatekeeper가 차단하면 운영체제의 명시적인 열기 기능을 사용하세요.

전용 파서는 CLI·GUI 실행 파일 옆에 있어야 합니다. Unix는 상대 실행 경로로 포함한
라이브러리를 찾고 Windows는 실행 파일 옆의 DLL을 사용합니다. 분석 엔진을 위한
환경 변수나 전역 라이브러리 설치는 필요하지 않습니다.

케이스·스캔 저널·내보내기 결과·증거 파일은 애플리케이션 폴더 밖에 저장하세요.
패키지를 교체하기 전에는 앱과 스캔을 모두 종료합니다. 새 패키지는 별도 폴더에 풀어
체크섬을 확인하고 기존 케이스의 사본을 열어 검증한 뒤 교체하세요. 확인이 끝날 때까지
이전 폴더를 유지합니다. 이 포터블 패키지는 애플리케이션 폴더를 지우면 제거되며,
외부에 저장한 케이스나 사용자 설정은 삭제하지 않습니다. 중단한 스캔은 원래 엔진의
지문에 연결되어 있으므로 엔진이 바뀐 뒤에는 새로 스캔해야 할 수 있습니다.

## 구성과 원본

압축 파일에는 `.sha256` 파일이 함께 제공됩니다. `package.json`에는 Git 커밋·대상 환경,
파일 크기·SHA-256·실행 파일 경로·네이티브 원본 리비전·실행 의존성을 기록합니다.
이 검사는 손상 검출용이며 배포자에 대한 신뢰 서명은 아닙니다. `licenses/`에는 네이티브·Rust
의존성 고지, 검토한 FFmpeg 구성과 의존성 잠금 파일을 포함합니다. Windows 런타임은
빌드 도구의 라이선스가 적용되는 REDIST 폴더에서 수정 없이 복사합니다. Microsoft의
[재배포 목록](https://learn.microsoft.com/visualstudio/releases/2026/redistribution)과
[앱 로컬 배포](https://learn.microsoft.com/cpp/windows/choosing-a-deployment-method)를 참고하세요.
앱 로컬 런타임의 갱신은 패키지 업데이트로 전달합니다.

대응하는 `MetaXtract-sources-<target>.tar.gz`에는 애플리케이션·빌드 원본과 실제로 사용한
정확한 네이티브 라이브러리 원본 트리를 넣습니다. 애플리케이션 원본에는 libarchive 필터
검사·할당 처리, qpdf 수정과 FFmpeg 빌드 방법이 포함되며, 상위 프로젝트 원본은 그대로
유지합니다. 공유 라이브러리는 교체할 수 있고 메타데이터용 외부 실행 프로그램은 포함하지
않습니다. 동봉한 네이티브 원본으로 다시 빌드하려면 `CMAKE_TOOLCHAIN_FILE`을
`upstream-sources.cmake`의 절대 경로로 설정하고 `application/`에서
`cargo build --workspace --release --locked`를 실행하세요. Cargo의 고정한 Rust 의존성을
받는 데 네트워크가 필요할 수 있습니다. 재빌드에는 일반 빌드 도구가 필요하지만 완성한
패키지의 실행에는 필요하지 않습니다.

## 패키지 제작

문서의 Rust·C++ 빌드 도구와 Python 3.11 이상을 준비한 깨끗한 Git 작업 폴더에서 실행합니다.

```bash
python packaging/build.py --check-source-archives
python packaging/build.py
```

제작기는 제품용 워크스페이스를 빌드하고 정확한 Cargo·네이티브 산출물을 찾아 OS 이외의
연결 의존성을 검사한 뒤 `dist/`에 실행 패키지와 대응하는 원본 압축 파일을 만듭니다.
Windows에는 Visual Studio의 x64 C++ 도구와 REDIST 파일이 필요하고 Unix에는
`readelf` 또는 `otool`이 필요합니다. `--allow-dirty`는 로컬 검증용이며 미커밋 상태를
구성 목록에 표시합니다. CI 배포 산출물에는 사용하지 않습니다. 교차 컴파일과 공용 코드
서명은 이 제작기의 범위에 포함하지 않습니다.

## 설치 검증

```bash
python packaging/verify.py --dist dist
python packaging/verify.py --dist dist --previous path/to/previous-artifact --gui-smoke
```

Python 검증기는 빌드·CI 도구이며 설치한 앱에 포함하지 않습니다. 공백·한국어가 있는
전용 폴더에서 도구가 없는 PATH를 사용하고 Cargo·Python·라이브러리 경로 환경 변수를
제외합니다. 압축 파일과 대응하는 원본을 검증하여 설치한 뒤 실제 PNG 메타데이터와
정해진 디코딩 픽셀을 확인하고, 교체 전후의 케이스 원본 내보내기·식별자·메모를 보존합니다.
실행 확인 실패 시 이전 앱을 복구하고, 알 수 없는 파일·폴더가 있으면 자동 교체·제거를
거부합니다. 생성한 감사 도입 이전 스키마 5 케이스를 이관하고, 미래 스키마는 쓰지 않고
거부하며, 제거 뒤에도 외부 증거·케이스·설정이 남는지 검사합니다. GUI 모드는 저장한
케이스를 실제로 열고 데스크톱의 최근 케이스 설정도 보존하는지 확인합니다.

별도의 새 CI 실행기에서 빌드한 패키지와 보관된 같은 OS·아키텍처의 이전 성공 패키지를
내려받습니다. 네이티브 빌드 폴더를 재사용하거나 Python 기준 앱을 설치하지 않습니다.
`installation-verification.json`에 두 커밋을 기록하고 이전 패키지에서의 업그레이드와
같은 패키지의 재설치를 구분합니다. 검증 보고서는 `Rust-installation-<OS>-<arch>` 산출물에, 실행 패키지는
`Rust-MetaXtract-<OS>-<arch>` 산출물에 제공합니다.
세 플랫폼 모두 패키지의 GUI를 실행하며 Linux는 Xvfb와 소프트웨어 렌더링을 사용합니다.
호스팅 실행기의 격리된 환경을 검증하는 것이며
모든 OS 이미지·GPU 드라이버·배포판을 검사했다는 의미는 아닙니다. 기존 Python 기준판의
CI는 동등성 확인·제거 단계까지 별도로 유지합니다.
