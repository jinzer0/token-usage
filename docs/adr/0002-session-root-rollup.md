# ADR 0002: GJC·Codex 서브에이전트 루트 합산

## 결정

파서는 사용량과 독립적인 세션 메타데이터를 반환하고, 공통 집계는 GJC·Codex의 명시적 부모 관계를 기간 필터 전에 해석한다. 원본 사용량의 귀속만 변경해 기존 tokscale 기반 accumulator에 정확히 한 번 전달한다. OpenCode의 기존 파서 단계 루트 정규화는 유지한다.

## 동인

- 사용량 없는 부모와 조회 기간 밖 조상을 계보에 보존한다.
- CLI·JSON·TUI가 하나의 루트 전용 스냅샷을 사용한다.
- 전체 토큰과 캐시·추론·모델·effort·사용 날짜를 보존하고 중복 합산을 피한다.
- 부모 증거를 추정하지 않고 다른 클라이언트의 같은 ID를 분리한다.

## 공식 Codex 계약

참조 revision은 `openai/codex`의 `72f999edc7848961b99c4b98bf1e62d12fa35798`이다.

- [SessionSource·SubAgentSource와 부모 접근자](https://github.com/openai/codex/blob/72f999edc7848961b99c4b98bf1e62d12fa35798/codex-rs/protocol/src/protocol.rs#L2954-L3200)
- [SessionMeta·SessionMetaLine](https://github.com/openai/codex/blob/72f999edc7848961b99c4b98bf1e62d12fa35798/codex-rs/protocol/src/protocol.rs#L3250-L3400)
- [thread 생성 경로](https://github.com/openai/codex/blob/72f999edc7848961b99c4b98bf1e62d12fa35798/codex-rs/thread-store/src/local/create_thread.rs)

`session_meta.payload.id`는 원래 세션 정체성이다. 직접 `parent_thread_id`와 `source.subagent.thread_spawn.parent_thread_id`만 명시적 부모 증거로 사용한다. `forked_from_id`는 일반 fork 이력이므로 합산 연결로 사용하지 않는다. 공유 `session_id`·분류·이름·시각·깊이도 관계 추정 근거가 아니다. 테스트 입력은 공식 구조에서 만든 합성 fixture이며 실제 사용자 로그 캡처라고 표현하지 않는다.

## 내부 전달과 예외 정책

`ParseResult.session_metadata`는 세션 존재·제목과 부모 후보 증거를 사용량 없이 전달한다. `SessionMetadata.parent_candidates`는 소스가 포함된 `SessionKey`를 키로 후보와 대표 파일·줄을 보존한다. 모든 메타데이터와 원래 usage 부모 증거를 합집합으로 수집한 후 연결을 결정한다. 제목 패치·부모 부재·null·후속 단일 후보는 기존 후보를 삭제하지 않는다.

- 후보가 없으면 독립 세션이다.
- 후보가 하나면 해당 연결을 사용한다.
- 후보가 여러 개면 나가는 부모 연결만 제외하고 세션당 한 번 경고한다. 그 노드의 존재·사용량·정상 자식 연결은 유지한다.
- 부모 로그가 없으면 마지막 존재 조상에 합산하고 경고한다.
- 순환 또는 순환으로 진입하는 세션은 원래 정체성으로 유지하고 경고한다.

반복 탐색과 메모이제이션을 사용하며 임의 깊이 제한이나 재귀를 추가하지 않는다. 계보 경고는 유효 usage를 건너뛴 것으로 세지 않고 warning 카운터에 정확히 한 번 반영한다.

## 보존 계약

메타데이터는 가짜 0토큰 레코드로 만들지 않으며 record 수·날짜·토큰·effort·추론 미상 상태에 기여하지 않는다. 포함된 실제 사용량만 합산한다. 루트 제목만 사용하고 자식 제목을 대체값으로 쓰지 않는다. ADR 0001의 시작 시각·필터 전 마지막 사용 시각·checked 산술·원래 모델·명시적 total 보존 계약을 유지한다. 자식 선택 별칭·별도 활성화 옵션·JSON 스키마 확장은 없다.

## 대안과 결과

소스별 정규화는 유효하지만 두 소스의 정책·진단이 중복되고 원래 파싱 정체성이 소실될 수 있어 선택하지 않았다. `SessionStats`의 사후 병합은 무사용 부모의 복구와 날짜·모델·effort 재병합이 필요해 중복 회계 위험이 크다.

공통 사전 귀속은 `ParseResult` 생성자 갱신과 집계 단계 경고 회계를 요구하지만 기존 accumulator와 표시 경로를 재사용한다. 새 의존성·범용 그래프 프레임워크·OpenCode resolver 이전은 필요 없다. 검증은 소스별 다단계·무사용 부모·고아·순환·충돌·일반 fork 부정 사례와 독립 수치 원장, 기간 필터, CLI·JSON·TUI 및 기존 OpenCode 회귀를 포함한다.
