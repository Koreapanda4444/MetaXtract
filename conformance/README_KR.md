# 언어 전환 검증 자료

[English](README.md) | [한국어](README_KR.md) | [프로젝트 README](../README_KR.md)

이 디렉터리는 범용 루트 테스트 폴더 대신 언어 전환에 꼭 필요한 검증 자료를 모읍니다.
입력 바이트와 비교할 결과는 재현 가능하게 고정합니다. 사용자의 비공개 증거 파일이나
사용자에게서 복사한 파일은 포함하지 않습니다.

검증에는 Rust 1.88, CMake 3.24 이상, C++20 컴파일러와 Python 기준판이 필요합니다.
Windows의 Visual Studio 2026은 CMake 4.2 이상이 필요하며 CI는 4.2.3을 사용합니다.
저장소 루트에서 실행합니다.

```bash
python conformance/run.py
python conformance/run.py --release
python conformance/validate.py --cargo
cmake --preset dev
cmake --build --preset dev --config Debug
ctest --preset dev -C Debug
```

비교 전에 파일 시스템 수정 시각처럼 실행 환경에 따라 바뀌는 필드는 제외합니다.
이 자료는 기존 동작의 호환성을 확인합니다. 네이티브 엔진이 새로 추출하는 더 깊은
메타데이터는 파일 형식 명세나 여러 파서의 교차 검증으로 확인하며,
Python이 기존에 추출한 얕은 결과로 범위를 제한하지 않습니다.

실행기는 Python 기준 레코드와 경로 동작을 비교한 뒤 샘플 바이트를 Rust, C++,
Rust가 소유하는 메모리·파일 읽기 콜백으로 전달합니다. 바이트 영역, 파일 끝의 짧은 읽기,
오프셋 오버플로, 읽기 상한, 잘린 파일, 오류 변환, 스키마 규칙과 모든 파서 자원 제한을
검사합니다. 샘플 파서는 Cargo의 명시적인 `conformance` 기능에서만 활성화되며
일반 제품 빌드에는 포함되지 않습니다.

생성된 샘플로 네이티브 이미지 구조 필드를 검증합니다. 바이트 순서 차이, CRC 오류,
디렉터리 순환 참조, 잘린 입력, 부분 결과와 원본 바이트의 출처도 확인합니다.
이미지 메타데이터 샘플은 EXIF 하위 디렉터리, 모르는 부호 있는 수·실수 태그,
XMP 네임스페이스·엔티티·깊이, IPTC, 여러 ICC 조각, PNG 압축 메타데이터,
썸네일·댓글과 압축 해제 상한도 검사합니다. 해석된 필드는 원본 패킷을 식별해야 하며
원본 파일 좌표를 임의로 주장할 수 없습니다.
HEIF·RAW 샘플은 실제 인코딩된 AVIF, 생성한 DNG, 항목·속성·영역의 경계, 제조사 태그,
외부 참조 거부와 라이브러리 작업 메모리 제한도 검사합니다.
PNG·PDF 등 실제 메타데이터 레코드의 전체 동등성은 아직 검증 전입니다.
실행 결과도 이 상태를 명시하며 Python만으로 만든 레코드를 네이티브 추출 지원 범위로
계산하지 않습니다. 네이티브 빌드 도구 없이 동결된 Python 기준판만 검사하려면
`--python-only`를 사용합니다.

CI는 Linux·Windows·macOS에서 빌드와 디버그·릴리스 호환성 검증을 수행합니다.
Linux에서는 네이티브 코어와 생성된 언어 연결부에 AddressSanitizer와
UndefinedBehaviorSanitizer도 적용합니다.
Rust 정책의 회귀 검증도 `cargo test --workspace --locked`로 실행합니다.

검증이 보호하는 규칙은 [아키텍처](../docs/architecture_KR.md),
[언어 경계](../docs/ffi_KR.md), [파서 자원 제한](../docs/resource-limits_KR.md)을 참고합니다.

오디오 샘플은 계획한 8개 형식 계열의 원본·라이브러리 값, 모르는 ID3 데이터, Vorbis·Opus
댓글, 청크·프레임 범위와 읽기·관측·할당 제한을 검사합니다. TagLib 원본 샘플은 자료에
정확한 커밋과 원본 라이선스 출처를 기록합니다.

영상 샘플은 계획한 모든 컨테이너 계열, 조각·큰 BMFF 박스, 스트리밍 EBML 경계, AVI
스트림 헤더, MPEG 영상 헤더, 전송 패킷 정렬·PSI CRC, 손상된 범위와 깊이·항목·읽기 제한을
검사합니다.

세부 샘플은 코덱 설정, 제목·챕터·언어, 원본·라이브러리 그림 바이트, 첨부물, 비ASCII
FourCC 전달, 시각 표시 비트와 잘못된 페이로드 범위를 검사합니다.

내부 미디어 검증은 실제 MP3·FLAC·Opus 프레임, 정해진 PCM 샘플, PNG 미리보기 픽셀,
AVI 프레임, 크기 축소, 미지원 코덱, 외부 재생목록 프로토콜 차단과 읽기·메모리·출력 상한을
검사합니다. 네이티브 빌드에는 Python 3.11 이상, GNU make와 셸도 필요합니다. Windows는
MSVC와 C:/msys64의 MSYS2 bash·make를 사용하거나 METAXTRACT_MSYS_ROOT로 지정합니다.
