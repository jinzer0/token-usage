# ADR 0001: tokscale 집계 로직의 정보 보존 이식

## 결정

tokscale revision `d4d1c751856e25913bce97bfbd7b254308863239`의 `crates/tokscale-core/src/aggregator.rs`를 기반으로 날짜·세션 accumulator 구조를 `src/aggregate/tokscale.rs`에 이식한다. 무수정 원형 재사용으로 표현하지 않는다.

## 동인

- 명시적 `u64` 총량, 캐시, 추론 미상, 원래 모델·effort 정보를 보존한다.
- 같은 ID를 가진 다른 클라이언트의 세션을 구분하고 정밀한 시각과 로컬 날짜를 유지한다.
- 원형 코드가 표현하지 못하는 정보를 가짜 토큰이나 보정 합계로 우회하지 않는다.

## 검토한 대안

1. `tokscale-core` 의존성: 원형 API를 호출할 수 있지만 signed 수치·파생 총량·모델 정규화와 무관한 가격/네트워크 의존성이 포함된다.
2. 원형 소스 선택 복사와 adapter: 부모·effort sidecar는 가능하지만 `u64::MAX`, total-only 기록, 원래 모델과 정밀·미상 시각을 모두 손실 없이 표현하지 못한다.
3. **소스 기반 명시적 이식**: 기존 타입과 오류 계약을 유지하며 실제 upstream fold/accumulator 흐름을 실행 경로에 사용한다.

## 심볼 대응과 변경

| upstream | 로컬 | 변경 |
|---|---|---|
| `aggregate_by_session` entry/fold | `aggregate_with_options` 세션 map | `SessionKey`로 클라이언트 구분, 기간 필터 전 마지막 시각 관찰 |
| `DailyFold::add/finish` | `DailyFold::add/finish` | 순차 `BTreeMap`, 로컬 `Option<NaiveDate>`, 날짜 미상 유지 |
| `DayAccumulator::add_message/merge/into_contribution` | 동일 역할의 메서드 | 명시적 `TokenStats` checked 합, 원래 모델·effort tuple, unknown OR |
| `SessionAccumulator` first/last tracking | `SessionAccumulator` | 정밀한 `Option<DateTime<Utc>>`, 부모와 이름 보존 |
| signed saturation / canonical model ID | 제외 | overflow 오류와 원래 모델 식별을 유지 |
| 비용·provider 문자열·intensity·graph·rayon | 제외 | 요구 범위 밖 기능과 의존성을 가져오지 않음 |

기존 독립 `UsageAccumulator`는 제거한다. 날짜별 참조는 세션 정렬이 끝난 snapshot 내부에 구성하며 모델 상세를 전역 날짜에 복제하지 않는다. CLI의 토큰순 저장 순서와 TUI의 최신순 view index는 분리한다.

## 결과와 유지보수

`TokenStats::checked_add_assign`은 모든 필드를 먼저 계산한 뒤 갱신하여 실패 시 부분 변경을 방지한다. 원형의 파생 총량과 달리 명시적 total-only 기록을 유지하며 추론을 출력에 다시 더하지 않는다. 소스별 입력 정규화는 parser가 담당한다. GJC 원시 `input`은 캐시와 별개이고, Codex 누적 알림의 반복은 중복 사용량으로 집계하지 않는다.

upstream 갱신은 immutable revision, 의미 차이 검토, 독립 기대값 및 실제 로그 대조를 거치는 별도 변경이다. 자동 다운로드나 런타임 fallback 엔진은 없다. 출처와 MIT 전문은 소스 헤더 및 `THIRD_PARTY_NOTICES`에 보존한다.

## 검증 조건

전체·날짜·세션·모델·effort 배분, 캐시·추론 미상, 최대 수치와 overflow, 로컬 날짜 경계, 기간 필터 밖 마지막 사용 시각을 독립 기대값으로 확인한다. 실제 로컬 로그도 동일한 고정 입력으로 대조한다. upstream 총합 일치나 표본 확인만으로 전체 검증 완료를 선언하지 않는다.
