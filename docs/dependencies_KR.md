# 의존성 정책

[English](dependencies.md) | [한국어](dependencies_KR.md) | [프로젝트 README](../README_KR.md)

[`third-party.lock.toml`](../third-party.lock.toml)은 검토한 네이티브 라이브러리와 언어 연결부
의존성을 기록하는 기준 파일입니다. 각 항목은 정확한 릴리스, 태그와 태그가 실제로 가리키는
커밋을 고정합니다. `python conformance/validate.py --online`은 주석 태그를 포함해
태그와 커밋이 일치하는지 확인합니다. 목록에 있다고 바로 사용되는 것은 아닙니다.
해당 형식의 구현 작업에서 필요한 어댑터를 추가하고 빌드 옵션을 확인하며 배포 시 포함할
고지 내용을 기록해야 합니다.

[`dependency-policy.toml`](../dependency-policy.toml)에서 허용한 카피레프트 라이브러리는
동적 라이브러리로 함께 배포해 라이선스와 교체 조건을 이행할 수 있도록 합니다.
FFmpeg는 GPL 또는 nonfree 구성 요소 없이 설정해야 합니다. libheif가 불러오는 코덱
플러그인은 디코딩을 활성화하기 전에 별도 검토가 필요합니다.

현재 프로젝트에는 GPL 호환 배포 정책이 없어 Exiv2를 선택하지 않았습니다.
이미지 추출은 특정 작업에 필요한 라이브러리와 MetaXtract 자체 어댑터부터 구성합니다.
새 제품의 실행 시 의존성으로 ExifTool과 ffprobe를 사용하는 것은 금지합니다.
동결된 Python 기준판에 관련 기능이 남아 있어도 새 제품에 포함되는 것은 아닙니다.

의존성을 갱신할 때는 버전과 커밋을 함께 변경하고 라이선스 검증을 다시 실행해야 합니다.
병합 전에 손상된 파일을 포함한 호환성 검증도 통과해야 합니다.

[`Cargo.lock`](../Cargo.lock)은 실제 사용하는 Rust 의존성과 레지스트리 체크섬을 고정합니다.
`python conformance/validate.py --cargo`는 간접 빌드 의존성을 포함해 패키지가 선언한
SPDX 라이선스를 검사합니다. 선언 검사이므로 실제 라이브러리를 활성화하는 어댑터를
추가할 때는 배포 고지와 네이티브 빌드 설정도 검토해야 합니다. Cargo는 단독 네이티브
빌드와 같은 CMake 타깃으로 추출 코어를 빌드한 뒤 생성된 `cxx` 연결부를 연결합니다.

저장소 루트에서 의존성 검증을 실행합니다.

이미지 메타데이터 어댑터는 검토한 pugixml 1.16과 zlib 1.3.2를 정적으로 포함합니다.
CMake는 잠금 파일의 정확한 커밋을 빌드 디렉터리에 받아 사용하며 시스템 라이브러리로
자동 대체하거나 실행 중 다운로드하지 않습니다. 설치 결과의 `share/metaxtract/licenses`에
원본 라이선스 고지를 포함합니다. 빌드 도구와 소스 다운로드는 빌드할 때 필요합니다.

RAW 어댑터는 LibRaw 0.22.2를 동적으로 포함하며 LGPL·CDDL 고지와 원본 저작권 목록을
함께 설치합니다. 고정한 소스는 선택적인 RawSpeed·DNG SDK·JPEG·색 관리 의존성 없이
빌드합니다. Cargo 실행은 설치된 네이티브 라이브러리 디렉터리를 사용합니다. 독립적인
Rust 앱을 배포할 때는 라이브러리와 고지도 포함해야 합니다. Libheif는 검토 목록에 있으나
아직 활성화하지 않았습니다. HEIF 메타데이터는 바이트 영역을 제한한 자체 박스·항목
파싱으로 추출하며 외부 픽셀 디코더를 사용하지 않습니다.

```bash
python conformance/validate.py --online
python conformance/validate.py --cargo
```

온라인 검증에는 네트워크 연결이 필요합니다. Cargo 검증에는 고정된 Rust 도구 모음과
의존성 메타데이터가 필요합니다. 빌드 환경은 [전환 검증](../conformance/README_KR.md)을 참고합니다.

오디오 어댑터는 TagLib 2.3.2를 동적으로 포함하고, UTFcpp 4.2.0 헤더도 정확한 커밋으로
고정합니다. UTFcpp의 Boost Software License 1.0을 검토해 허용 목록에 추가했습니다.
Git 하위 모듈을 암묵적으로 받지 않으며, 시스템 zlib·UTFcpp 대신 검토한 소스를 사용합니다.
설치 결과에 원본 LGPL·MPL·UTFcpp 라이선스를 포함합니다.

미디어 어댑터는 FFmpeg 9.0.2의 avformat·avcodec·avutil·swscale·swresample 공유
라이브러리를 함께 배포합니다. 빌드 전용 Python 코드가 고정 소스와 검토한 zlib로 빌드합니다.
GNU make와 셸이 필요하며 Windows는 MSVC와 MSYS2 bash·make를 사용합니다. 새 제품
실행에는 Python이나 메타데이터 실행 프로그램이 필요하지 않습니다. 설치 결과에 원본
LGPL 안내와 실제 설정을 포함합니다. 캐시 키에는 소스·zlib 리비전, 컴파일러, CRT와
sanitizer 설정을 반영하고 Cargo 구성마다 동일한 소스 경로를 사용합니다.

PDF 어댑터는 고정한 qpdf 라이브러리·내장 암호 처리 소스와 검토한 libjpeg-turbo를 정적으로
배포합니다. JPEG SIMD·실행 도구·TurboJPEG는 비활성화하고 시스템 zlib·JPEG·OpenSSL·GnuTLS를
자동 선택하지 않습니다. 원본 Apache·IJG·libjpeg 안내를 설치 결과에 포함합니다. 생성한
qpdf Pl_Buffer 사본에는 파서별 자원 제한 확인을 한 곳 추가하며 원본 리비전과 수정 내용을
명시합니다. 아래 문구는 IJG 원본 배포 요구 사항입니다.

This software is based in part on the work of the Independent JPEG Group.
