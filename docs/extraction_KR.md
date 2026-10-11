# 네이티브 추출 지원 범위

[English](extraction.md) | [한국어](extraction_KR.md) | [프로젝트 README](../README_KR.md)

## 콘텐츠 식별

Rust의 `metaxtract_native::detect_content` API는 자원 제한이 적용된 읽기 전용 입력을 통해
C++ 시그니처 판별기를 호출합니다. 최대 앞부분 64 KiB를 읽고 인식한 후보를 모두 반환합니다.
각 후보에는 MIME, 컨테이너, 근거가 있는 바이트 영역과 판별 상태가 포함됩니다.

| 상태 | 의미 |
|---|---|
| Signature | 식별 바이트를 인식했지만 구조의 유효성은 확인하지 않음 |
| HeaderValidated | 명시한 헤더 검사를 통과함. 전체 파일의 유효성을 뜻하지 않음 |
| Truncated | 인식한 헤더 또는 선언된 영역이 실제 입력 범위를 초과함 |
| Invalid | 시그니처는 있지만 헤더 필드가 서로 모순됨 |

JPEG, PNG, TIFF·BigTIFF·CR2, WebP, GIF, BMP, ICO·CUR, HEIF·HEIC·AVIF, RIFF 오디오·영상,
AIFF, FLAC, Ogg, ID3·MPEG 오디오, APE, WavPack, ISO BMFF·QuickTime, EBML,
MPEG 프로그램·전송 스트림, PDF, ZIP, OLE 시그니처를 인식합니다. ZIP 패키지의 세부 형식,
Ogg 코덱, EBML DocType과 개별 메타데이터는 더 깊은 파서가 필요합니다.
ID3 헤더만으로 뒤에 있는 데이터가 MP3 오디오임을 확정하지 않습니다.

인식되는 시그니처가 없으면 빈 후보 목록을 반환합니다. 읽기 실패나 자원 제한 초과는
알 수 없는 파일로 성공 처리하지 않고 오류로 반환합니다. 파일 시작점 밖의 시그니처와
여러 후보를 함께 보존하며 임의로 하나만 정답으로 선택하지 않습니다.

자원 예산은 [파서 자원 제한](resource-limits_KR.md), 정상·잘린·모순된·시그니처가 섞인 입력의
검증은 [전환 검증](../conformance/README_KR.md)을 참고합니다.

## 형식 충돌

Rust의 `metaxtract_core::assess_content`는 판별 후보를 확장자, 선언된 MIME,
선언된 컨테이너와 비교합니다. 원래 후보를 유지하면서 불일치, 여러 시그니처,
잘리거나 모순된 헤더, 인식하지 못한 콘텐츠와 미확인 세부 형식을 보고합니다.
대소문자와 흔히 쓰는 MIME 별칭을 정규화합니다. ZIP 파일의 DOCX 확장자나 EBML 파일의
WebM 확장자만으로 세부 형식을 확정하지 않으며 깊은 파서가 확인할 때까지 미확인으로 남깁니다.
ID3도 뒤에 있는 오디오 형식을 확정하지 않습니다. 범용 바이너리 MIME은 불필요한 충돌을 만들지 않습니다.

## 이미지 구조

네이티브 `images.structure` 파서는 JPEG 프레임·스캔·세그먼트, CRC와 순서를 검사한
PNG 청크, 일반 TIFF·BigTIFF 디렉터리, WebP 헤더·애니메이션 영역, GIF 프레임·제어 블록,
BMP DIB와 ICO·CUR 디렉터리·내장 이미지를 추출합니다. TIFF는 모르는 태그와 원본 값도
보존합니다. 항목 위치와 내장 이미지 범위는 자신을 포함하는 원본 영역 안에 있어야 하며,
디렉터리 순환 참조는 거부합니다.

`metaxtract_native::extract_parser`는 자료형을 구분한 값, 원본 바이트, 바이트 영역,
자원 사용 통계와 선택적인 실패 정보를 반환합니다. 복구 가능한 실패 전에 읽은 관측값은
계속 확인할 수 있습니다. 공통 스키마로 변환할 때 원본 바이트와 해석값을 따로 보존합니다.
구조·메타데이터 검사이며 압축된 픽셀을 디코딩해 이미지 전체의 유효성을 보증하지는 않습니다.
EXIF 하위 디렉터리, XMP·IPTC·ICC 해석과 추가 이미지 메타데이터는 다음 작업입니다.
