# MetaXtract

[English](README.md) | [한국어](README_KR.md)

MetaXtract는 로컬 파일의 메타데이터를 추출하고 분석 결과의 근거를 보존하는 프로그램입니다.
많은 파일 형식을 지원하면서 메타데이터를 깊게 추출하고 정확하게 해석하는 것이 목표입니다.
성능과 메모리 효율도 중요하지만 분석 범위와 정확도를 희생하지 않습니다.

현재 Python에서 Rust + C++로 전환 중입니다. 언어의 역할은 작업에 따라 나눕니다.
C++는 바이너리 파싱·디코딩·원본 메타데이터 추출을 맡고, Rust는 앱 운영과 결과 분석을 맡습니다.

## 현재 상태

전환 중에도 Python 기준판의 GUI와 CLI를 사용할 수 있습니다. Rust/C++ 쪽은 공통 데이터
스키마, 언어 연결부, 제한된 입력 읽기, 파서 등록부, 자원 제한, 호환성 검증과 여러 운영체제의
CI가 구현되어 있습니다. 실제 파일 형식용 파서 어댑터와 Rust GUI·CLI는 아직 구현 전입니다.

Python 소스는 결과 비교를 위한 기준판으로 동결합니다. 새 앱이 기존 사용 흐름, 데이터 호환성,
추출 결과 검증을 통과한 뒤 제거할 예정입니다.

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
| [`native/`](native/README_KR.md) | C++ 추출 코어와 향후 네이티브 라이브러리 어댑터 |
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
| 언어 전환 검증 | [Conformance](conformance/README.md) | [전환 검증](conformance/README_KR.md) |
| 현재 GUI·CLI 사용법 | [Python application](legacy/python/README.md) | [Python 앱 사용법](legacy/python/README_KR.md) |
