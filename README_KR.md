# MetaXtract

[English](README.md) | [한국어](README_KR.md)

MetaXtract는 로컬 파일의 메타데이터를 추출하고 분석 결과의 근거를 보존하는 프로그램입니다.
많은 파일 형식을 지원하면서 메타데이터를 깊게 추출하고 정확하게 해석하는 것이 목표입니다.
성능과 메모리 효율도 중요하지만 분석 범위와 정확도를 희생하지 않습니다.

현재 Python에서 Rust + C++로 전환 중입니다. 언어의 역할은 작업에 따라 나눕니다.
C++는 바이너리 파싱·디코딩·원본 메타데이터 추출을 맡고, Rust는 앱 운영과 결과 분석을 맡습니다.

## 현재 상태

전환 중에도 Python 기준판의 GUI와 CLI를 사용할 수 있습니다. Rust/C++ 엔진은 제한된 입력과
스키마·언어 연결부, 파일 식별, 심층 이미지·RAW·오디오 메타데이터, 영상 컨테이너·트랙 상세
분석과 내부 검사·디코딩을 구현했습니다. PDF 문서 정보·XMP·페이지 크기·첨부·개정 이력,
암호화 설정·서명 범위·동작 구조도 추출합니다. Rust는 이미지 관측값을 교차 검증합니다.
OOXML·ODF 패키지 속성·관계와 수정 이력·주석 메타데이터도 추출합니다.

`document.ole`에서 OLE 복합 파일과 구형 Word·Excel·PowerPoint의 디렉터리 정보, 문서 속성, 매크로·내장 스트림 목록을 추출합니다. 분산된 스트림은 물리적 조각과 복원 스트림 식별자를 기록하고, 지원하지 않는 암호화·문자 인코딩은 명시적으로 구분합니다.

`email.mime`은 EML/MIME 헤더, 순서가 유지된 전달 경로 주장, 본문, 첨부와 중첩 메시지를 추출합니다. 중복 항목·원본 바이트를 유지하고 RFC2047/RFC2231과 일반 전송 인코딩을 해독하며, 인증·서명은 실제 검증하지 않았다고 표시합니다.

ZIP·TAR·7z와 일반 압축 스트림은 네이티브 라이브러리로 읽습니다. Rust는 공통 한도 안에서
압축파일·첨부·OLE 스트림을 재귀 분석하고 가상 증거 경로를 보존합니다. 실행파일·스크립트,
글꼴·전자책·패키지, DICOM과 지리정보 어댑터는 원시 메타데이터를 유지하며 미지원 인코딩과
변환을 명시합니다. 알 수 없는 필드와 백엔드별 독립 주장을 보존합니다.

Rust 코어는 안전한 파일 탐색, SHA-256·BLAKE3 스트리밍 해싱, 제한된 병렬 스캔,
콘텐츠 기반 캐시, SQLite 점진 저장, 지속 가능한 일시정지·재개와 전용 파서 프로세스
격리를 제공합니다. 입출력 버퍼를 재사용하고 제한된 작업 창에서 비용에 따라 순서를
조정하며 완료한 기록을 짧은 트랜잭션으로 묶어 저장합니다.

Rust [케이스 저장 라이브러리](docs/cases_KR.md)는 단일 파일 케이스, 손실 없는
메타데이터 저장, 인덱스 기반 페이지 조회, 이력·주석, Python 케이스·JSONL·서명
번들 가져오기, 해시 체인 감사, 암호화 서명 키와 검증 가능한 이동식 스냅샷을
제공합니다. 네이티브 파서를 불러오지 않고 케이스를 열 수 있습니다.

Rust [분석 라이브러리](docs/analysis_KR.md)는 정규화, 필드 출처·타임라인, 해석 교차 검증,
발견 사항, 동일·내용·시각 유사도, 오프라인 위치 분석, 스캔·필드 비교, 구조화 검색,
비식별화 미리보기, 선택 CSV·JSONL·증거 내보내기, 서명 영수증을 지원하는 HTML·JSON
조사 보고서를 제공합니다. 현재 라이브러리 API이며 이후 앱 단계에서 GUI·CLI에 연결합니다.

Rust CLI에서 스캔·검사·비교·검증·케이스 작업을 사용할 수 있습니다.
데스크톱 연결과 전체 사용 흐름의 동등성 확보는 이후 작업입니다.
지원 필드와 한계는 [추출 지원 범위](docs/extraction_KR.md)를 참고합니다.

Python 소스는 결과 비교를 위한 기준판으로 동결합니다. 새 앱이 기존 사용 흐름, 데이터 호환성,
추출 결과 검증을 통과한 뒤 제거할 예정입니다.

새 CLI·Rust 데스크톱 사용법은 [Rust 앱 안내](docs/application_KR.md)를 참고합니다.

## 현재 앱 실행

Python 3.11 이상을 설치하고 저장소 루트에서 실행합니다.

```bash
python -m pip install -e ./legacy/python
metaxtract gui
```

GUI에는 Python의 Tk 지원이 필요합니다. CLI 도움말은 `metaxtract --help`로 확인합니다.
설치, 지원 형식, 명령과 제한 사항은 [Python 앱 사용법](legacy/python/README_KR.md)에 있습니다.

새 Rust/C++ 제품은 ExifTool이나 ffprobe 같은 외부 메타데이터 실행 프로그램에 의존하지 않습니다.
허용된 네이티브 라이브러리는 연결하거나 앱과 함께 배포합니다. 동결된 Python 기준판에는
선택적인 ffprobe 비디오 기능이 남아 있지만 새 아키텍처에 포함되지는 않습니다.

## Rust/C++ 기반 빌드

고정된 Rust 1.88.0 도구 모음, CMake 3.24 이상, C++20 컴파일러를 사용합니다.
Windows에서 Visual Studio 2026을 사용한다면 CMake 4.2 이상이 필요하며 CI는 4.2.3을 사용합니다.

네이티브 빌드에는 Python 3.11 이상, GNU make와 셸도 필요합니다. Windows는 MSVC와
MSYS2 bash·make를 사용하며 다른 설치 경로는 METAXTRACT_MSYS_ROOT로 지정합니다.

검증 전에 위 명령으로 Python 기준판을 설치합니다. 저장소 루트에서 실행합니다.

```bash
cargo build --workspace --locked
python conformance/run.py
```

이 명령은 언어 전환 기반을 빌드하고 검증합니다. Rust 데스크톱 앱을 실행하거나 아직 구현하지
않은 형식 어댑터의 메타데이터 추출 동등성을 확인하는 명령은 아닙니다.

## 저장소 구조

| 경로 | 역할 |
|---|---|
| [`crates/`](crates/README_KR.md) | Rust 스키마, 네이티브 연결부와 앱 기반 |
| [`native/`](native/README_KR.md) | C++ 원시 파서·네이티브 라이브러리 어댑터·제한된 디코딩 |
| [`legacy/python/`](legacy/python/README_KR.md) | 실행 가능한 Python 앱과 동결된 소스 기준판 |
| [`reference/python/`](reference/python/README_KR.md) | Python 앱에서 기록한 기존 동작 기준 |
| [`conformance/`](conformance/README_KR.md) | 전환용 샘플과 호환성 검증 |
| `docs/` | 설계 이유, 의존성 정책과 네이티브 코드의 안전성 규칙 |

## 문서 안내

README는 처음 실행하는 방법을 안내합니다. `docs/`는 추출 엔진을 수정할 때 알아야 할
설계 이유와 제약을 설명합니다. 프로젝트 안내 문서는 영어판과 한국어판을 함께 제공하며,
동작이나 설계가 바뀌면 두 문서를 함께 갱신합니다.

| 안내 문서 | English | 한국어 |
|---|---|---|
| 아키텍처와 역할 분담 | [Architecture](docs/architecture.md) | [아키텍처](docs/architecture_KR.md) |
| 언어 경계와 메모리 소유권 | [Rust/C++ boundary](docs/ffi.md) | [Rust/C++ 경계](docs/ffi_KR.md) |
| 라이브러리와 의존성 정책 | [Dependencies](docs/dependencies.md) | [의존성 정책](docs/dependencies_KR.md) |
| 파서 자원 제한과 한계 | [Resource limits](docs/resource-limits.md) | [파서 자원 제한](docs/resource-limits_KR.md) |
| 네이티브 추출 지원 범위 | [Extraction](docs/extraction.md) | [추출 지원 범위](docs/extraction_KR.md) |
| 케이스 작업 공간 | [Cases](docs/cases.md) | [케이스](docs/cases_KR.md) |
| 분석·검색·보고서 | [Analysis](docs/analysis.md) | [분석·검색·보고서](docs/analysis_KR.md) |
| 언어 전환 검증 | [Conformance](conformance/README.md) | [전환 검증](conformance/README_KR.md) |
| 현재 GUI·CLI 사용법 | [Python application](legacy/python/README.md) | [Python 앱 사용법](legacy/python/README_KR.md) |
