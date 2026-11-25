import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

// Loads the module the webview uses, with its real number formatting.
async function views(t) {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
  t.after(() => vite.close());
  return vite.ssrLoadModule('/src/commandViews.ts');
}

const counts = (over = {}) => ({ requests: 0, input: 0, output: 0, cache_read: 0, reasoning: 0, unpriced_requests: 0, observed_cost_usd: null, ...over });

test('cost shows the session, the latest turn and each model when more than one ran', async (t) => {
  const { costView } = await views(t);
  const reply = {
    observed_cost: 1.5,
    usage: {
      total: {
        all: counts({ requests: 3, input: 12000, output: 800, cache_read: 4000, observed_cost_usd: 1.5 }),
        workers: counts({ requests: 1, observed_cost_usd: 0.25 }),
        by_model: {
          'anthropic/big': { main: counts({ requests: 2, input: 9000, observed_cost_usd: 1.25 }), workers: counts() },
          'local/small': { main: counts(), workers: counts({ requests: 1, input: 3000, unpriced_requests: 1 }) },
        },
      },
      turn: { all: counts({ requests: 1, input: 500, observed_cost_usd: 0.004 }) },
    },
  };
  const { rows, text } = costView(reply);
  assert.match(text, /not an invoice/);
  assert.deepEqual(rows.map((r) => r.title), ['This session · $1.50', 'Latest turn · <$0.01', 'Workers · $0.25', 'anthropic/big · $1.25', 'local/small · not priced']);
  assert.equal(rows[0].detail, '3 requests · 12k in · 800 out · 4.0k from cache');
  assert.match(rows[4].detail, /^1 request · 3\.0k in .* 1 without a price$/);
});

test('cost from an engine without usage groups falls back to the one number it has', async (t) => {
  const { costView } = await views(t);
  assert.deepEqual(costView({ cost: 0.5 }), { text: 'Observed session cost: $0.50' });
  assert.deepEqual(costView(null), { text: 'Observed session cost: —' });
});

test('context shows how full the window is and what was sent', async (t) => {
  const { contextView } = await views(t);
  const reply = { messages: 1, metrics: { compactions: 2, prompt_tokens: 50000, output_tokens: 900, cached_tokens: 0, responses: 7, reasoning_tokens: 300 } };
  const { rows } = contextView(reply, { tokens: 50000, window: 200000, estimated: true });
  assert.equal(rows[0].title, 'In context · 50k tokens of 200k (25%)');
  assert.match(rows[0].detail, /Estimated/);
  assert.equal(rows[1].title, 'Conversation · 1 message');
  assert.equal(rows[1].detail, 'Compacted 2 times so far.');
  assert.equal(rows[2].title, 'Model replies · 7');
  assert.equal(rows[2].detail, '50k in · 900 out · 0 read from cache · 300 reasoning');
  // No reported context yet: the window row is left out, nothing is invented.
  assert.equal(contextView({}, null).rows[0].title, 'Conversation · 0 messages');
});

test('status names the session, model, approval, tools and folders', async (t) => {
  const { statusView } = await views(t);
  const tools = Array.from({ length: 16 }, (_, i) => `t${i}`);
  const { rows } = statusView({
    title: 'Fix login', session_id: 's1', approval: 'ask', plan_mode: true, tools, observed_cost: 0,
    current_model: { provider: 'anthropic', model_id: 'big', thinking: { level: null, default_level: 'high' } },
    workspace: { root: '/work/app', dirs: ['/work/lib'] },
  });
  assert.deepEqual(rows.map((r) => r.title), ['Session · Fix login', 'Model · anthropic/big', 'Approval · ask', 'Tools · 16 available', 'Project · /work/app', 'Observed cost · $0.00']);
  assert.equal(rows[1].detail, 'Reasoning: high');
  assert.match(rows[2].detail, /Plan mode is on/);
  assert.match(rows[3].detail, /t13 and 2 more$/);
  assert.equal(rows[4].detail, 'Also: /work/lib');
  assert.equal(statusView({}).rows[1].title, 'Model · none chosen');
});

test('collaboration describes no room, a hosted room and a joined room', async (t) => {
  const { collabView } = await views(t);
  const none = collabView({ room: { role: 'none', joining: false, notices: [] }, local_hosts: [] });
  assert.match(none.text, /No room is open/);
  assert.deepEqual(none.rows, []);

  const host = collabView({
    room: { role: 'host', read_only: true, peers: [{ id: 'p1', name: 'Sam', writable: false }], notices: ['Sam joined'] },
    local_hosts: [{ model: 'big', cwd: '/work/app', guests: 1, read_only: true }],
  });
  assert.equal(host.text, 'Sam joined');
  assert.deepEqual(host.rows.map((r) => r.title), ['Hosting a room · guests can read only', 'Sam', 'On this computer · big · read only']);
  assert.equal(host.rows[0].detail, '1 guest connected.');
  assert.equal(host.rows[2].detail, '/work/app · 1 guest');

  const guest = collabView({ room: { role: 'guest', read_only: false, admitted: true, ended: false, replicated_entries: 2, question: { id: 1 } } });
  assert.equal(guest.rows[0].title, 'Joined as a guest · you can steer');
  assert.equal(guest.rows[0].detail, '2 entries received.');
  assert.equal(guest.rows[1].title, 'The host is asking a question');
  assert.equal(collabView({ room: { role: 'guest', admitted: false } }).rows[0].detail, 'Waiting for the host to let you in.');
});
