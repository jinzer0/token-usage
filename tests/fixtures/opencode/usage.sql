INSERT INTO session(id, parent_id, title) VALUES
    ('abc', NULL, 'Root without usage'),
    ('child', 'abc', 'Child'),
    ('grandchild', 'child', 'Grandchild'),
    ('sibling', 'abc', 'Sibling');
INSERT INTO message VALUES
    ('first', 'child', 0, '{"role":"assistant","providerID":"anthropic","modelID":"model","time":{"created":0,"completed":1},"finish":"stop","tokens":{"input":100,"output":40,"reasoning":7,"cache":{"read":20,"write":3},"total":170}}'),
    ('second', 'grandchild', 1000, '{"role":"assistant","providerID":"anthropic","modelID":"model","time":{"created":1000,"completed":1001},"tokens":{"input":10,"output":5,"reasoning":1,"cache":{"read":4,"write":2},"total":22}}'),
    ('interrupted', 'sibling', 2000, '{"role":"assistant","providerID":"other","modelID":"model","time":{"created":2000},"tokens":{"input":2,"output":3,"reasoning":0,"cache":{"read":0,"write":0}}}'),
    ('completed-zero', 'child', 3000, '{"role":"assistant","providerID":"anthropic","modelID":"model","time":{"created":3000,"completed":3001},"tokens":{"input":0,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}'),
    ('placeholder', 'child', 4000, '{"role":"assistant","providerID":"anthropic","modelID":"model","time":{"created":4000},"tokens":{"input":0,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}'),
    ('user', 'child', 6000, '{"role":"user","providerID":"anthropic","modelID":"model","time":{"created":6000},"tokens":{"input":9000,"output":9000,"reasoning":9000,"cache":{"read":9000,"write":9000}}}');
UPDATE session SET tokens_input = 9000, tokens_output = 9000, tokens_reasoning = 9000,
    tokens_cache_read = 9000, tokens_cache_write = 9000 WHERE id = 'child';
INSERT INTO part VALUES
    ('step-one', 'first', 'child', '{"type":"step-finish","tokens":{"input":999999,"output":999999,"reasoning":999999,"cache":{"read":999999,"write":999999}}}'),
    ('step-two', 'first', 'child', '{"type":"step-finish","tokens":{"input":999999,"output":999999,"reasoning":999999,"cache":{"read":999999,"write":999999}}}');
