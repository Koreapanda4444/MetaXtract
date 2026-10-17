# Rust 앱

[English](application.md) | [한국어](application_KR.md)

명령행 앱과 전용 파서를 함께 빌드합니다.

```bash
cargo build -p metaxtract-cli -p metaxtract-worker --locked
cargo run -p metaxtract-cli --locked -- --help
cargo run -p metaxtract-cli --locked -- scan /path/to/evidence --case investigation.mxcase --progress
cargo run -p metaxtract-cli --locked -- case history investigation.mxcase
cargo run -p metaxtract-cli --locked -- verify investigation.mxcase
```

Windows에서는 `.exe`를 붙입니다. Cargo가 빌드한 `metaxtract-worker`와 검토된 네이티브
라이브러리를 앱 옆에 유지합니다. `inspect FILE`은 독립된 임시 케이스를 만들고 `compare CASE
OLD_SCAN NEW_SCAN`은 저장된 스캔을 비교합니다. 케이스 명령으로 생성·정보·이력·노트를
다룹니다. 출력은 JSON이며 스캔 진행은 stderr로 보냅니다. 운영체제 고유 경로 단위를 보존합니다.

결과는 단일 파일 케이스에 점진적으로 저장하고 실행 중 체크포인트는 별도 `.scan.sqlite`
저널에 유지합니다. 일시정지 중에는 저널도 보존해야 합니다. 같은 원본·케이스와
`--resume RUN --scan CASE_SCAN_ID`로 재개하고 저널 경로를 바꿨다면 `--journal PATH`도
지정합니다. 완료 결과와 원자적 영수증을 대조해 중단 구간을 복구하며 중복 저장을 방지합니다.
저널과 케이스는 다른 파일이어야 하며 출력 파일은 탐색에서 제외합니다.

## 분석과 결과 전달

`scan` 또는 `case history`가 반환하는 스캔 ID를 사용합니다.

```bash
cargo run -p metaxtract-cli -- search investigation.mxcase SCAN_ID --query 'mime:png' --limit 128
cargo run -p metaxtract-cli -- export investigation.mxcase SCAN_ID selected.csv --format csv
cargo run -p metaxtract-cli -- redact investigation.mxcase SCAN_ID private.jsonl
cargo run -p metaxtract-cli -- report investigation.mxcase SCAN_ID report.html --analyst Analyst
cargo run -p metaxtract-cli -- analyze investigation.mxcase SCAN_ID --view timeline
cargo run -p metaxtract-cli -- bundle export investigation.mxcase snapshot.zip
cargo run -p metaxtract-cli -- bundle open snapshot.zip reopened.mxcase
```

검색 문법은 [분석 안내](analysis_KR.md)를 참고하세요. `--cursor`에는 이전 페이지의 `next` JSON을
넣습니다. `--save NAME`은 구조화된 필터를 저장하고 `--order path|size|insertion`은 검색·내보내기
정렬을 지정합니다. `redact --preview RECORD_ID`는 파일을 쓰지 않고 변경 내용을 보여 줍니다.
`--profile FILE`로 기존 비식별화 JSON 프로필을 적용할 수 있으며 원본은 바꾸지 않습니다.
`import CASE INPUT --kind jsonl|python|python-bundle`로 이전 스캔과 Python 케이스를 가져옵니다.

`keygen PRIVATE PUBLIC --name NAME --password-stdin`은 암호화된 Ed25519 개인키를 만듭니다.
비밀번호는 표준 입력으로 전달하며 명령 인수로는 받지 않습니다. 자동화에서는
`METAXTRACT_KEY_PASSWORD`도 사용할 수 있습니다. 공개키 PEM은 독립적으로 신뢰할 수 있게 보관하세요.
`sign INPUT SIGNATURE --key PRIVATE --password-stdin`은 정확한 원본 바이트를 서명하고
`verify-signature INPUT SIGNATURE PUBLIC`은 이를 검증합니다. 번들 내보내기와 보고서에도 같은 키
옵션을 사용합니다. 서명한 보고서는 `--receipt FILE`이 필수이며 다이제스트 목록과 서명을 별도
보관합니다. 기존 출력 파일은 덮어쓰지 않습니다. 번들의 내부 무결성과 서명자 신뢰는 구분하며,
독립적으로 신뢰하는 키로 인증하려면 `--public-key PUBLIC`을 지정합니다.

## 명령 출력 규약

성공하면 표준 출력에 `{"ok":true,"command":"search","result":...}` JSON 객체 하나를 씁니다.
`--pretty`는 공백만 바꿉니다. 도움말은 사람이 읽는 텍스트입니다. 오류는 표준 오류에
`ok:false`, `error.kind`, `error.message`, `error.exit_code`가 있는 JSON 객체로 출력하며
표준 출력은 비웁니다. `--progress`는 `{"event":"scan","data":...}` JSON을 표준 오류에 한 줄씩
출력합니다. 알 수 없는 옵션·중복 옵션·초과 인수는 케이스를 수정하기 전에 거부합니다.
파일 경로는 운영체제 원래 표현을 지원하며 검색 문구는 UTF-8이어야 합니다.

종료 코드: **0** 완료, **2** 잘못된 인수, **3** 입출력·애플리케이션 오류, **4** 데이터·무결성
검증 오류, **5** 실패·불안정 파일이 있는 스캔 완료, **6** 안전하게 일시 정지, **130** 취소입니다.
부분 완료·정지도 성공 결과 객체에 집계와 체크포인트 ID를 남깁니다. 출력 파이프가 닫히면
정상 종료합니다. `case history`와 `search`는 `items`와 `next`를 반환하며 케이스가 변경되면
페이지 탐색을 처음부터 다시 시작해야 합니다. `--pause-after N`은 최소 N개 파일 완료 후
협력적으로 정지하므로 이미 처리 중이던 파일은 추가로 완료될 수 있습니다. 저널을 보관하고
반환된 run·scan ID로 재개하세요. 운영체제에서 강제 중단되어도 저널로 복구할 수 있습니다.

## Rust 데스크톱

```bash
cargo build -p metaxtract-worker --locked
cargo run -p metaxtract-gui --locked
cargo run -p metaxtract-gui --locked -- investigation.mxcase
```

네이티브 데스크톱에서 케이스 생성·열기, 최근 케이스, 읽기 전용 열기, 스캔 기록, 제목·노트를
사용합니다. 수정은 로컬 케이스에 저장됩니다. 복사 저장은 일관된 휴대용 스냅샷을 만들고
검증한 뒤 복사본을 열며 원래 케이스를 보존합니다. 제한된 경로 선택기는 다른 프로그램을
실행하지 않고 직접 입력과 운영체제 원래 경로 표현의 항목 선택을 지원합니다. 최근 경로는
운영체제 MetaXtract 설정 디렉터리에 저장합니다.

한국어 표시는 Windows의 맑은 고딕, macOS의 Apple SD Gothic Neo, Linux의 Noto Sans CJK·
나눔고딕을 사용합니다. 필요하면 설치된 TrueType/OpenType 폰트 경로를 `METAXTRACT_FONT_PATH`로
지정하세요. Linux는 데스크톱 디스플레이, OpenGL과 libxkbcommon의 X11 실행 라이브러리
(Ubuntu/Debian의 `libxkbcommon-x11-0`)가 필요합니다. `--smoke`는 실제 창을 띄워
여러 프레임을 렌더링한 후 종료하는 설치 확인용 옵션입니다. 케이스 작업은 백그라운드에서
수행해 화면 응답을 유지하며 기존 앱 아이콘을 보존합니다.

개발 시 `cargo run`은 네이티브 라이브러리 검색 경로를 자동 설정합니다. 독립 실행 파일을
배포할 때는 `metaxtract-worker`와 [의존성 안내](dependencies_KR.md)의 공유 라이브러리·고지를
함께 구성해야 합니다.

### 실시간 스캔 대기열

쓰기 가능한 케이스를 열고 **스캔 대기열**에서 파일·폴더를 찾아 추가합니다. 최대 64개
원본을 대기시키며 스캔마다 1–32개 작업자를 사용합니다. 파일 완료마다 결과를 저장하면서
진행률과 집계를 갱신합니다. **일시 정지**는 재개 가능한 체크포인트를 보존하고 나머지
대기열도 정지합니다. **스캔 취소**는 현재 스캔을 취소 상태로 마칩니다. 대기열 제거는
아직 시작하지 않은 원본에만 적용합니다. 케이스를 다시 열고 **체크포인트 재개**로
정지·중단된 스캔을 이어갈 수 있으며, 영수증의 운영체제 원래 경로로 원본·저널을 찾아
해당 세션과 일치하는지 검증합니다. 창을 닫으면 먼저 스캔을 정지하고 체크포인트 저장을
기다립니다. 아직 시작하지 않은 대기열은 현재 실행 동안만 유지하며 진행 중인 스캔의
체크포인트는 별도 저널에 보존합니다.

### 페이지 결과와 다중 선택

**레코드**는 페이지마다 가벼운 헤더 64개를 읽으며 경로·크기·발견 순 정렬과 이전·다음 이동을
지원합니다. 케이스가 바뀌어 기존 커서를 사용할 수 없으면 첫 페이지로 돌아갔다고 알립니다.
스캔 중에는 첫 페이지를 갱신하며 행을 열 때 해당 레코드 하나의 전체 데이터만 읽습니다.
체크박스 또는 Ctrl/Cmd+클릭으로 페이지에 걸쳐 최대 64개 ID를 선택하고 선택 개수를 표시합니다.
선택 CSV·JSONL은 같은 경로가 중복되어도 선택한 스캔 안의 정확한 ID만 내보냅니다.
**선택 주석 교체**는 선택한 레코드 전체의 노트·태그를 입력 내용으로 교체하며 하나의
감사 트랜잭션으로 저장합니다. 다른 묶음을 처리하려면 선택을 해제하세요. 구조화 CLI 검색도
`id=NUMBER`로 정확한 레코드를 선택할 수 있습니다.

### 원본 검증 미리보기

레코드를 열고 원본 파일 또는 별도로 추출한 중첩 멤버를 선택합니다. 일반 파일의 크기와
SHA-256이 기록과 일치해야 디코딩합니다. 최대 64 MiB를 읽고 16진수는 64 KiB, 텍스트는
32 KiB까지 표시하며 제한된 임시 스냅샷과 전용 파서 프로세스를 사용합니다. 가져온 케이스에
적힌 경로를 자동으로 따라가지 않습니다.

JPEG·PNG·BMP·GIF·TIFF·WebP와 지원되는 미디어 코덱은 C++에서 첫 RGB 프레임을 디코딩합니다
(긴 변 최대 256픽셀). 오디오는 첫 프레임의 디코딩 정보를 보여 줍니다. 미지원 코덱과
적용하지 못한 내장 표시 변환은 명시합니다. OOXML·ODF는 제한된 본문 텍스트를 표시합니다.
PDF는 처음 3페이지에서 Standard-14 라틴 글꼴의 ASCII 문자열을 내용 순서대로 표시합니다.
사용자 정의 글꼴·비ASCII 문자는 명시적으로 생략하며 페이지 배치를 렌더링하지 않습니다.
전체 원본 메타데이터는 케이스에 유지하며 표시 한도로 변경하지 않습니다. 암호화·미지원
입력도 검증한 16진수와 진단을 볼 수 있습니다. CLI의 `preview CASE RECORD_ID FILE`도 같은
기능입니다. 파서 빌드가 바뀌어 엔진 지문이 다른 체크포인트는 재개하지 않으므로 추출 엔진을
변경한 경우 새 스캔을 시작하세요.

스캔 결과의 `result.errors`에는 처음 64개 파일 오류 요약을 넣습니다. 전체 실패 수는
`summary.failed`에, 파일별 전체 영수증은 케이스에 보존합니다. 진행 이벤트도 `data.error`를
포함합니다. 전용 파서의 지문이 다르면 앱과 파서 실행 파일을 함께 다시 빌드해야 한다고
안내합니다.

### 레코드 검사

선택한 레코드에 미리보기와 함께 **원본·정규화·출처·충돌·진단** 탭을 제공합니다. 원본 관측에는
원래 값·인코딩 바이트, 파서 식별자, 패킷·스트림·바이트 범위를 보존하고 해석 값을 별도로
표시합니다. 정규화 해석에는 신뢰도·해결 상태·지지 및 반대 관측 ID를 보여 줍니다. 가져온
레코드는 메모리 복사본에서 분석하며 실패해도 원본을 볼 수 있습니다. 필드 출처를 정확한
관측에 연결하고 충돌의 대안을 누르면 해당 해석으로 이동합니다. 큰 목록은 보이는 행만
그립니다. 상세 JSON은 화면마다 16 KiB, 바이너리는 128바이트까지 표시하며 전체 값은 케이스와
내보내기에 유지합니다.

### 검색과 비교

**검색·비교**에서 선택한 스캔에 구조화 검색을 적용합니다. 공백 AND, 따옴표 값, `-` 부정을
지원하며 선택 사항인 구조화 JSON 편집기에서 제한된 AND·OR·NOT 트리를 사용할 수 있습니다.
MIME·상태·정규화 키·태그별 집계는 더 있는 그룹을 명시합니다. 이름으로 저장한 필터는
전체 구조화 조건을 케이스에 보존하며 불러올 때 선택한 스캔에 적용합니다. 이전 필터의
원문도 보존하고 알 수 없는 상태나 문법은 직접 검토해야 한다고 안내합니다.

비교 탭에 이전·이후 스캔 ID와 두 스캔에 공통 적용할 조건을 넣습니다. 실제 비교한 ID·조건·
케이스 리비전을 보존하며 추가·삭제·변경·이동을 구분합니다. 변경 레코드를 열고 필드를
선택하면 이전·이후 값과 의미 단위 해석 변경을 볼 수 있습니다. 바이너리 변경은 긴 바이트
배열 대신 길이·SHA-256으로 표시합니다. 일관된 비교를 위해 진행 중인 스캔을 정지해야 하며
작업 중 리비전이 바뀌면 비교를 중단합니다.

### 분석 화면

**분석**에는 케이스 전체 통계, 리비전 기준 요약, 근거와 연결된 타임라인, 오프라인 경도·위도
도표, 정확한 해시 중복 그룹, 시각적 유사성, 심각도별 발견 사항이 있습니다. 현재 레코드 필터나
선택 ID가 분석 범위를 정합니다. 파일 완료·실패 통계는 스캔 상태와 한 트랜잭션으로 저장하므로
다시 열어도 유지됩니다. 이전·가져온 스캔에는 통계가 없을 수 있습니다. GeoJSON/KML은 유효한
전체 위치를 내보냅니다. 도표는 처음 5,000개를 그리며 목록에서는 전체 위치를 확인할 수 있습니다.
잘못된 좌표는 진단에 남깁니다. 경로 연결에는 사용자가 지정한 경로별 그룹과 확정된 시간대가
필요하며 임의로 이동 경로를 추정하지 않습니다.

새 스캔에서 **미리보기 디코딩**을 켜면 네이티브 디코더의 제한된 이미지 지문을 저장해 시각적으로
비교할 수 있습니다. 지문이 없는 항목은 분석 누락으로 표시합니다. 유사성·이상 표시는 검토 단서이며
같은 작성자이거나 변조되었다는 증거로 단정하지 않습니다.

## 내보내기·도구

데스크톱 **내보내기·도구**에서 현재 필터·선택 ID의 CSV·JSONL, 사용자 비식별화 프로필의
미리보기·저장, 한영 HTML·JSON 보고서와 비교 기준 스캔을 사용할 수 있습니다. 비식별화는
파생본을 만들며 저장된 원본은 보존합니다. 보고서를 서명하려면 별도의 새 영수증 경로가
필요합니다. **보고서 영수증 검증**은 별도로 신뢰하는 공개 PEM을 사용해 보고서 바이트와
서명을 확인합니다. CLI 명령은 `verify-report REPORT RECEIPT PUBLIC_PEM`입니다.

암호화 Ed25519 키, 파일 바이트 서명·검증을 지원하며 암호 입력은 가리고 작업 시작 시 지웁니다.
암호는 설정에 저장하지 않습니다. 공개 키에 이름을 붙여 쓰기 가능한 케이스에 등록할 수 있습니다.
전체 이동식 번들은 레코드 필터·비식별화 설정과 관계없이 원본 메타데이터·노트·주석을 포함하며
원본 근거 파일은 포함하지 않습니다. 번들을 다시 열 때는 새 케이스 경로를 지정합니다. JSONL,
Python 케이스·번들을 기존 쓰기 가능한 케이스로 가져올 수 있습니다.

감사 검증은 별도로 보관한 앵커를 선택적으로 사용하고, 현재 앵커 저장과 순서 번호 기준 활동
조회(요청당 128개)를 제공합니다. 파서 진단에는 현재 빌드 지문, 파서·라이브러리 버전, 워커 유무와
저장된 스캔 엔진 정보를 표시합니다. 엔진이 바뀌면 다른 파서로 체크포인트를 재개하지 않고
새로 스캔해야 합니다.

작업은 백그라운드에서 실행합니다. 출력은 새 경로를 사용하고 기존 파일은 덮어쓰지 않습니다.
보고서·번들·가져오기·케이스 전환을 실행하려면 진행 중인 스캔을 일시정지하세요.

## 조사 작업 순서

1. 네이티브 패키지 전체를 풀고 데스크톱을 실행해 앱 폴더 밖에 케이스를 만듭니다.
   원본 근거는 별도 디렉터리에 보관하고 사용한 패키지의 빌드 커밋을 기록합니다.
2. **스캔 대기열**에 원본을 추가합니다. 시각 유사도가 필요하면 **미리보기 디코딩**을
   켭니다. 스캔 진행·일시정지 중에는 케이스와 `.scan.sqlite` 저널을 함께 보존합니다.
3. 정규화된 주장을 쓰기 전에 레코드 상태·진단·Raw·Provenance를 확인하고 **분석**에서
   충돌과 추출 범위 누락을 검토합니다. 필터·선택은 작업 대상을 정하며 다른 기록을 지우지 않습니다.
4. 보고서·스냅샷 제작 전 스캔을 일시정지합니다. 내부 검토용 원본 레코드 내보내기를 보존하고
   필요하면 별도의 비식별화 파생본을 미리 봅니다. 메타데이터 비식별화는 원본 파일 내부의
   정보를 제거하는 파일 정화 기능이 아닙니다.
5. 케이스 스냅샷·원본 근거·별도로 신뢰한 공개 키/보고서 영수증을 보존합니다. 스냅샷을 새
   케이스로 다시 열어 검증한 뒤 작업 사본을 정리합니다. 이동식 케이스 스냅샷에는 데이터베이스가
   들어가며 원본 근거는 별도로 관리합니다.

아래 `metaxtract`는 실행 파일의 전체 경로, Unix의 `./metaxtract`, Windows PowerShell의
`.\metaxtract.exe`로 바꿉니다. 명령이 반환한 ID를 다음 작업에 사용합니다.

```text
metaxtract scan evidence --case investigation.mxcase --progress
metaxtract case history investigation.mxcase
metaxtract search investigation.mxcase SCAN_ID --limit 64
metaxtract verify investigation.mxcase
metaxtract report investigation.mxcase SCAN_ID report.html --analyst Analyst
metaxtract bundle export investigation.mxcase snapshot.zip
metaxtract bundle verify snapshot.zip
metaxtract bundle open snapshot.zip reopened.mxcase
```

**이전 케이스 가져오기:** 쓰기 가능한 Rust 케이스를 만들고 **케이스로 가져오기**에서
Python 케이스·JSONL·Python 번들 중 입력에 맞는 종류를 선택합니다. CLI는
`case create new.mxcase` 뒤에 `import new.mxcase old.mxc --kind python`을 실행합니다.
반환한 스캔 ID를 확인하고 대상 케이스를 `verify`로 검사합니다. 원본 케이스는 유지하며
가져온 원본 행과 지원하지 않는 필터 문법도 검토할 수 있습니다. 서명된 Python 번들에는
별도로 신뢰한 공개 키가 필요합니다.

**중단된 스캔 복구:** 원래 케이스·저널·원본 루트를 유지한 뒤 **체크포인트 재개**를
선택합니다. CLI는 원래 `--resume RUN --scan SCAN_ID`와 사용자 지정 `--journal PATH`를
사용합니다. 엔진이 다르면 새 스캔이 필요합니다. 저널을 잃어도 저장된 케이스 기록은
읽을 수 있지만 사라진 체크포인트를 복원할 수는 없습니다.

## 원본 검증, 증거 전달과 캐시

```bash
metaxtract scan evidence --case investigation.mxcase --include-hidden --decode-previews --cache on
metaxtract verify-originals investigation.mxcase SCAN_ID evidence
metaxtract export-evidence investigation.mxcase SCAN_ID evidence selected.zip --query 'path=evidence.txt'
metaxtract verify-evidence selected.zip
metaxtract analyze investigation.mxcase SCAN_ID --view locations --format geojson --output locations.geojson
metaxtract analyze investigation.mxcase SCAN_ID --view similar --threshold 8
metaxtract doctor --case investigation.mxcase
metaxtract cache purge investigation.mxcase.cache.sqlite
```

데스크톱 Workflows 메뉴에서도 원본 검증, 선택 증거 내보내기, 증거 ZIP 무결성 검증과 캐시
비우기를 실행할 수 있습니다. 스캔 대기열은 숨김 파일 포함, 파일 개수 제한과 캐시 사용을
설정합니다. 스캔 중 미리보기 디코딩은 선택 사항이며 저장한 디코딩 프레임으로 시각적 유사성을
분석합니다. 미리보기 부재와 방향 충돌은 명시적인 분석 범위 누락으로 표시합니다.

사용자가 원본 루트를 지정해야 하며 케이스에 기록된 경로만으로 임의의 디스크 접근을 허용하지
않습니다. 원본 검증은 지정 루트 안의 일반 파일을 다시 열고 심볼릭 링크를 거부하며 기록된 크기와
SHA-256을 비교합니다. 중첩된 가상 레코드는 별도로 집계하고 OS 경로로 열지 않습니다. 원본 검증을
완료했지만 불일치가 있으면 stdout 결과의 `verified`가 false이고 종료 코드는 4입니다.
선택 증거 내보내기는 실제 루트 파일만 받으므로 중첩 레코드는 먼저 필터로 제외하세요.

증거 ZIP에는 형식화된 메타데이터와 수정하지 않은 원본 바이트가 들어갑니다. 원본에 내장된 개인
정보도 그대로 포함됩니다. 메타데이터 가리기 내보내기 및 케이스 스냅샷과 구분됩니다.
`verify-evidence`는 파일을 풀지 않고 정해진 전체 목록과 해시를 검증합니다. 이는 내부 무결성
검증이며 출처 인증에는 `sign`과 별도로 신뢰한 공개 키를 사용하세요.

변경하지 않은 케이스를 반복 내보내면 고정 ZIP 시각을 사용합니다. 스냅샷 `created_ms`는 내보낸
현재 시각이 아니라 케이스 내용의 마지막 수정 시각입니다. 별도 용량 제한 캐시는 파일 내용·엔진·
정책에 따라 원본 추출 결과를 보관합니다. `--cache off`로 우회할 수 있습니다. 캐시 비우기는
기존 전용 캐시 DB만 허용하며 케이스 레코드와 스캔 체크포인트는 보존합니다.
