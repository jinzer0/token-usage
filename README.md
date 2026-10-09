# token-usage

로컬에 저장된 Codex·GJC 세션 로그와 OpenCode SQLite 데이터베이스를 읽어 세션별/기간별 토큰 사용량을 집계하는 CLI 도구입니다.

## Quick Install

Rust toolchain 없이 최신 GitHub Release 바이너리를 설치합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jinzer0/token-usage/main/install.sh | sh
```

기본 설치 경로는 `$HOME/.local/bin`입니다. 다른 경로를 쓰려면:

```bash
TOKEN_USAGE_INSTALL_DIR=/usr/local/bin \
  sh -c "$(curl -fsSL https://raw.githubusercontent.com/jinzer0/token-usage/main/install.sh)"
```

설치 후 확인:

```bash
token-usage --version
```

## Manual Install

1. https://github.com/jinzer0/token-usage/releases 에서 자신의 OS/architecture에 맞는 archive를 다운로드합니다.
2. 압축을 풉니다.
3. `token-usage` 바이너리를 `$PATH`에 포함된 디렉터리로 복사합니다.

예:

```bash
curl -LO https://github.com/jinzer0/token-usage/releases/download/v0.4.0/token-usage-v0.4.0-aarch64-apple-darwin.tar.gz
tar -xzf token-usage-v0.4.0-aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
mv token-usage ~/.local/bin/
token-usage --version
```

Release에는 `SHA256SUMS`가 포함되며, `install.sh`는 가능한 환경에서 checksum을 검증합니다.

## 지원 범위

- Codex 로그: 기본 경로 `~/.codex`
  - `sessions/**/*.jsonl` 스캔
  - `session_index.jsonl`의 세션 이름 반영
- GJC 로그: 기본 경로 `~/.gjc/agent/sessions`
  - `**/*.jsonl` 스캔
  - 중복 assistant usage 메시지 제거
  - 부모 세션 정보와 branch별 reasoning effort 반영
- OpenCode SQLite: 기본 경로 `~/.local/share/opencode/opencode.db`
  - `XDG_DATA_HOME`이 설정되면 `$XDG_DATA_HOME/opencode/opencode.db`
  - `OPENCODE_DB`는 절대 경로 또는 OpenCode 데이터 디렉터리 기준 상대 경로
  - `--opencode-db <FILE>`로 정확한 DB 파일 경로 지정 가능
  - 모든 하위 세션을 최상위 부모에 한 번만 합산하며 모델과 메시지 사용일은 보존
  - 비용 계산과 OpenCode 전용 화면은 제공하지 않음
- 출력 형식
  - 기본 텍스트 요약
  - 세션 상세 보기
  - 기간별 집계 테이블
  - JSON 출력
  - TUI 보기

## 개발 환경 실행

```bash
cargo run -- [OPTIONS] [SESSION]
```

릴리스 바이너리 빌드:

```bash
cargo build --release
./target/release/token-usage [OPTIONS] [SESSION]
```

## 사용법

```text
token-usage [OPTIONS] [SESSION]
```

### 주요 옵션

| 옵션 | 설명 |
| --- | --- |
| `--client <all|codex|gjc|opencode>` | 스캔할 클라이언트 선택. 기본값은 `all` |
| `--today` | 로컬 timezone 기준 오늘 record만 포함 |
| `--since <7d|30d|YYYY-MM-DD>` | 지정 시점 이후 record만 포함 |
| `--until <YYYY-MM-DD>` | 지정 날짜까지의 record만 포함. 해당 날짜 전체를 포함 |
| `--group-by <day|week|month>` | 기간별 bucket 테이블 출력 |
| `--json` | 집계 결과를 pretty JSON으로 출력 |
| `--verbose` | 캐시 토큰, record 수 등 추가 정보 출력 |
| `--tui` | 터미널 UI로 결과 표시 |
| `--codex-home <PATH>` | Codex 홈 경로 override |
| `--gjc-home <PATH>` | GJC 세션 로그 루트 override |
| `--opencode-db <FILE>` | OpenCode SQLite 파일 경로 override |
| `[SESSION]` | 특정 세션 상세 출력. 세션 id, prefix, 이름 prefix, `client:id` 사용 가능 |

`--json`과 `--tui`는 동시에 사용할 수 없습니다.

### OpenCode 회계와 읽기 안전성

```bash
token-usage --client opencode --today --verbose
token-usage --client opencode --opencode-db /path/to/opencode.db --json
token-usage --client opencode --tui
token-usage --client opencode opencode:ROOT_SESSION_ID
```

정상 하위 세션은 최상위 부모의 ID와 이름으로 표시합니다. 하위 ID에 대한 별도 선택 별칭은 없습니다. 기간 밖에 생성되거나 자체 사용량이 없는 부모도 계보에 유지하며, 오늘 조회는 **오늘의 메시지 사용량만** 부모 아래 합산합니다. 실제 부모가 없는 고아는 독립 루트로 유지합니다. 순환 계보는 진단과 함께 원래 세션 ID로 보존하여 무한 탐색이나 중복 집계를 막습니다.

설치본 OpenCode **1.18.35**의 저장 코드를 확인한 정규화:

- 저장된 `input`은 캐시를 제외한 입력, `output`은 추론을 제외한 출력입니다.
- `input_total = input + cache.read + cache.write`
- `output_total = output + reasoning`이며 추론을 총합에 다시 더하지 않습니다.
- `total_tokens = input_total + output_total`
- 모델은 `provider/model`로 구분합니다. 구분자와 `%`는 escape하여 이름 충돌을 막습니다.
- assistant 메시지당 저장된 사용량을 한 번만 계산합니다. 같은 사용량을 담은 `step-finish` part와 세션 요약은 추가 합산하지 않습니다.
- 필수 토큰 필드가 없거나 유효하지 않으면 추정하지 않고 진단 후 건너뜁니다. 유효한 사용량이 저장된 중단 메시지는 포함합니다.
- 날짜는 메시지 `time.created`의 밀리초 시각을 사용합니다. 유효한 시각이 없는 레코드는 기존 날짜 필터 정책을 따릅니다.

확인 근거는 설치 바이너리 SHA-256 `8c3c351b138cfe35905ab11846a1373f1beea590aee7eda412fb765b72c79d82`의 Bun JavaScript입니다. `getUsage` 정규화는 byte offset `67578535`, 통계 합계는 `65185836`/`65185927`, 메시지와 part 저장은 `65900134`, 메시지 생성 루프는 `65954500`–`65957500`에서 확인했습니다. 다른 저장 형식으로 자동 전환하지 않습니다.

DB는 SQLite의 읽기 전용 연결과 일관된 읽기 transaction으로 조회합니다. 원본 내용 수정, 마이그레이션, 강제 checkpoint, 복사 또는 쓰기 권한으로 재시도하지 않습니다. 실행 중 WAL의 committed 데이터도 읽지만, SQLite의 공유 메모리(`-shm`) 조정까지 포함한 **파일시스템 쓰기 0회**를 보장한다는 뜻은 아닙니다.

DB 누락·손상·읽기 실패와 행별 오류는 진단으로 표시합니다. 기본 경로 누락은 warning, 명시한 파일 누락은 error이며 다른 유효한 소스 결과는 유지합니다. SQLite 스캔 카운터는 파일 1개, 줄 0개로 기록합니다. OpenCode 또는 `all` 선택에서 선택된 레코드의 토큰 합계가 `u64`를 넘으면 집계 전에 명시 오류로 종료하며 값을 포화시키거나 조용히 버리지 않습니다.

DB 전체나 대화 본문을 메모리에 올리지 않지만, 세션 계보와 메시지별 토큰 메타데이터는 유지하므로 메모리는 레코드 수에 비례합니다. 큰 DB는 초기 조회와 TUI 새로고침이 오래 걸릴 수 있습니다. 시간 제한보다 정확성과 읽기 안전성을 우선합니다.

## 예시

전체 요약:

```bash
token-usage
```

오늘 사용량:

```bash
token-usage --today
```

최근 7일 사용량:

```bash
token-usage --since 7d
```

최근 30일을 일별로 집계:

```bash
token-usage --since 30d --group-by day
```

특정 날짜 범위:

```bash
token-usage --since 2026-09-01 --until 2026-09-10
```

월별 집계:

```bash
token-usage --group-by month
```

GJC 로그만 요약:

```bash
token-usage --client gjc
```

상세 토큰 정보를 포함한 요약:

```bash
token-usage --verbose
```

특정 세션 상세 보기:

```bash
token-usage gjc:01K4EXAMPLESESSIONID
```

JSON으로 내보내기:

```bash
token-usage --json --since 7d
```

테스트 fixture 같은 별도 경로를 스캔:

```bash
token-usage --codex-home tests/fixtures/codex --gjc-home tests/fixtures/gjc
```

TUI 실행:

```bash
token-usage --tui
```

## 기간 필터 정책

- 기간 필터가 없으면 timestamp가 없는 record도 기존처럼 포함합니다.
- `--today`, `--since`, `--until` 중 하나라도 활성화되면 timestamp가 없는 record는 기간 집계에서 제외됩니다.
- 제외된 timestamp 없는 record 수는 `source_counts.records_missing_timestamp_filtered`에 기록됩니다.
- 범위 밖 record 수는 `source_counts.records_time_filtered`에 기록됩니다.
- `--today`와 날짜 기반 필터는 사용자의 local timezone 기준입니다.

## 기본 텍스트 출력

요약 출력은 세션별로 다음 값을 보여줍니다.

```text
client session total input output reasoning
```

- `client`: `codex`, `gjc` 또는 `opencode`
- `session`: 세션 이름이 있으면 이름, 없으면 세션 id
- `total`: 총 토큰 수
- `input`: 입력 토큰 수
- `output`: 출력 토큰 수
- `reasoning`: 확인된 reasoning 토큰 수
  - `+`가 붙으면 일부 레코드에서 reasoning 토큰이 명시되지 않았다는 뜻입니다.

`--group-by`를 사용하면 기간별 테이블을 출력합니다.

```text
date         total       input       output      reasoning   records
2026-09-08    1,210,430     850,220     210,310     149,900       12
```

## 세션 상세 출력

`SESSION` 인자를 넘기면 해당 세션의 모델별, reasoning effort별 사용량을 보여줍니다.

세션 선택자는 다음 방식으로 해석됩니다.

1. `client:id`와 정확히 일치
2. 세션 id 전체 또는 prefix
3. 세션 이름 전체 또는 prefix

여러 세션이 동시에 매칭되면 ambiguous 에러와 후보 목록을 출력합니다.

## JSON 구조

`--json` 출력의 최상위 구조는 다음 필드를 포함합니다.

- `generated_at`: 집계 생성 시각
- `totals`: 현재 필터 범위 전체의 정확한 토큰 합계
- `sessions`: 세션별 집계 목록
- `timeline`: `--group-by` 사용 시 기간 bucket 목록
- `periods`: TUI/요약용 today, 7 days, 30 days token totals
- `diagnostics`: 파싱 중 발생한 경고/오류
- `source_counts`: 스캔 파일 수, 읽은 줄 수, emit/skip/filter record 수, missing/empty root 등

`sessions`는 총 토큰 수 내림차순으로 정렬됩니다. 각 세션의 `last_used_at`는 기간 필터 적용 전 유효한 사용 기록의 마지막 시각이며, 시각이 없으면 `null`입니다. 날짜별 인덱스는 TUI 내부 데이터로 JSON에 중복 저장하지 않습니다.

집계는 tokscale의 날짜·세션 accumulator 구조를 기반으로 이식했습니다. 참조 revision은 `d4d1c751856e25913bce97bfbd7b254308863239`이며 출처와 MIT 전문은 `THIRD_PARTY_NOTICES`에 있습니다. 원형의 signed/포화 합산 대신 명시적 `u64` 합계와 checked 오류 처리를 사용하고, 모델·추론 강도·캐시·추론 미상 정보를 보존합니다. 합계 overflow는 어느 소스를 선택하든 오류로 보고하며 부분 snapshot을 출력하지 않습니다.

Codex의 `session_meta`/`turn_context`/`event_msg.token_count` 원시 로그를 읽고, 같은 누적 사용량이 반복되는 알림은 다시 더하지 않습니다. GJC의 원시 `input`은 캐시와 별개이므로 입력 총량에는 `cacheRead`와 `cacheWrite`를 포함합니다. 추론 토큰을 출력에 다시 더하지 않으며 명시된 총량은 별도로 보존합니다.

## TUI

```bash
token-usage --tui
```

두 탭으로 구분합니다. `1 Sessions`는 세션별 전체 사용량과 모델·추론 상세, `2 Dates`는 왼쪽 날짜별 합계, 가운데 선택 날짜의 세션, 오른쪽 해당 세션의 당일 상세를 표시합니다. 날짜는 모달 없이 즉시 전환됩니다.

Dates 탭은 최소 **110열 × 12행**을 요구합니다. 그보다 작으면 확대 안내를 표시하고 선택은 그대로 보존합니다. 정상 크기로 돌아오면 같은 날짜와 세션을 조회할 수 있습니다.

주요 키:

```text
1 / 2          Sessions / Dates 탭
Tab/Shift-Tab   패널 이동 (←/→도 가능)
j/k 또는 ↑/↓   현재 패널의 날짜·세션 이동
[ / ]          Dates 탭에서 이전·다음 날짜
J/K            상세 패널 스크롤
/              세션 검색
v              상세 breakdown 토글
d              선택 세션을 고정해 Dates 탭 열기
p              날짜 탐색의 세션 고정 켜기·끄기
s              최신순 / 토큰 사용량순 전환
r              새로고침
?              도움말
q              종료
```

두 탭의 기본 정렬은 세션 전체의 마지막 사용 기록 최신순입니다. 시작 시각이나 파일 수정 시각을 사용하지 않으며, 시각 미상은 마지막에 둡니다. `s`의 토큰순은 Sessions에서 현재 기간 전체, Dates에서 선택 날짜의 사용량 기준입니다. 동률은 세션 ID와 클라이언트로 안정적으로 정렬합니다. 기간 필터가 있어도 최신순의 시각은 필터 전 전체 기록 기준입니다.

Dates의 여러 날짜 행에서 전체 합계를 비교할 수 있습니다. 증감은 이전 사용일 기준이며, 기록이 없는 날짜를 가짜 0일로 채우지 않습니다. 토큰이 0이어도 유효한 기록이 있는 날짜는 표시하고, 날짜 미상 기록은 `Unknown date`에서 조회합니다. `d` 또는 `p`로 세션을 고정하면 그 세션의 사용 날짜를 빠르게 넘길 수 있고, 가운데 목록은 계속 그날 전체 세션의 비교를 제공합니다. 다른 세션을 선택하면 고정을 해제합니다.

검색은 세션 목록만 필터하며 날짜의 전체 합계를 바꾸지 않습니다. 새로고침과 탭 전환은 유효한 날짜·세션·정렬을 유지하고, 사라진 선택은 안내와 함께 복구합니다. 새로고침 실패 시 이전 데이터와 선택을 유지합니다. 검색 입력 중 명령 문자는 검색 내용이며 Enter로 확정하거나 Esc로 취소합니다. OpenCode 날짜는 기존 최상위 부모 귀속을 유지한 메시지 사용 시각을 따릅니다.

## 개발

검증 명령:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

현재 통합 테스트는 다음 동작을 검증합니다.

- Codex fixture 파싱과 index 기반 세션 이름 반영
- GJC duplicate message 제거와 parent session 유지
- malformed JSONL, 누락 필드, invalid timestamp, unknown reasoning effort, empty/missing directory
- Codex cumulative stale/reset 처리
- 기간 필터와 day/week/month grouping
- `--json`과 `--tui` 동시 사용 runtime validation
- 잘못된 `--since`, `--group-by`, inverted range 처리
- OpenCode SQLite 계층·모델·날짜·누락/손상·WAL 읽기·갱신 및 합계 overflow
- 세션별 로컬 날짜·날짜 미상 분할, 날짜별 세션 참조와 합계 보존
- 두 탭·3열 최소 크기·세션 고정·최신순/당일 토큰순·선택 유지

## Release

- `.github/workflows/ci.yml`: PR/main push에서 fmt, clippy, test 실행
- `.github/workflows/release.yml`: `v*` tag push 시 release archive와 `SHA256SUMS` 생성

### 0.4.0 변경 사항

- Sessions/날짜 탭과 날짜·세션·모델 상세 3열 탐색, 날짜 비교 및 세션 고정
- 전체 마지막 사용 시각과 선택 날짜 사용량에 따른 정렬, 검색·새로고침·리사이즈 선택 유지
- tokscale 파생 집계와 checked 산술, 모델·추론 강도별 정확한 수치 및 날짜 미상·영 토큰 보존
- Codex 원시 payload 및 GJC 캐시 정규화 보완, 배포 archive에 MIT 제3자 고지 포함

지원 archive:

```text
token-usage-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
token-usage-vX.Y.Z-aarch64-apple-darwin.tar.gz
token-usage-vX.Y.Z-x86_64-apple-darwin.tar.gz
token-usage-vX.Y.Z-x86_64-pc-windows-msvc.zip
```

모든 archive에는 binary, README와 `THIRD_PARTY_NOTICES`가 포함됩니다. `LICENSE` 파일이 있으면 함께 포함됩니다.

## 제한 사항

- 로컬 JSONL 로그와 OpenCode SQLite만 읽습니다. 원격 API나 계정 사용량 페이지는 조회하지 않습니다.
- 로그 포맷이 바뀐 경우 일부 레코드는 diagnostic warning과 함께 건너뜁니다.
- Codex cumulative counter는 증가분으로 계산하며, stale/reset으로 보이는 값은 중복 방지를 위해 skip합니다.
- Linux arm64 release archive는 아직 기본 release matrix에 포함하지 않았습니다.
