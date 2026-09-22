# kdow — The Web App (kdow.space)

**An AI team workspace where non-technical professionals get a team of AI
agents that execute real work tasks. Not a chatbot. Not an agent builder.**

Target users: accountants, real estate agents, content creators, marketing
agencies, consultants, freelancers. NOT developers. These are people who
manage projects in spreadsheets and hire VAs at $2–5K/mo. kdow replaces
that with AI agents at $25–49/mo.

## Relationship to the kdo factory

The factory (docs 00–08) is infrastructure. kdow is the **consumer
product** that uses a simplified version of that architecture:

- The task board **is** the spec system (visual, no YAML)
- The agents **are** the data plane (API-only, no worktrees in v1)
- The memory **is** the memory plane (SQLite, single tier)
- No control plane daemon in v1 — the API server handles scheduling

We are not building the full factory yet. We are building a web app that
makes money. The factory architecture informs the design but does not
dictate the implementation.

## Where it sits in the stack

```
Browser (Next.js 15 on Vercel)
    │
    ▼ (HTTPS JSON + WebSocket)
Rust API Server (Axum on Fly.io)
    │
    ├──> Anthropic API     (Claude Sonnet 4.6 work, Haiku 4.5 routing)
    ├──> OpenAI API        (optional fallback)
    ├──> SQLite            (users, workspaces, agents, tasks, memory)
    └──> LemonSqueezy      (subscription webhooks)
```

The Rust backend is the only thing that talks to LLMs and Lemon Squeezy.
Frontend never sees an API key.

## The product UX

### Not a chat. A project board.

```
INBOX              WORKING             REVIEW              DONE
(you assign)       (agents working)    (needs your ok)     (completed)

┌─────────────┐    ┌─────────────┐     ┌─────────────┐    ┌─────────────┐
│ Research    │    │ Scout is    │     │ Email draft │    │ Competitor  │
│ competitors │    │ analyzing   │     │ ready for   │    │ report      │
│ in DeFi     │    │ 3 sources…  │     │ your review │    │ delivered   │
│ lending     │    │ [progress]  │     │ [Approve]   │    │ Apr 29      │
└─────────────┘    └─────────────┘     │ [Edit]      │    └─────────────┘
┌─────────────┐    ┌─────────────┐     │ [Redo]      │
│ Draft Q2    │    │ Builder is  │     └─────────────┘
│ investor    │    │ writing     │
│ update      │    │ section 2…  │
└─────────────┘    └─────────────┘
```

Each card shows title, agent + avatar, progress, time elapsed. Click to
expand to the full conversation thread.

### First-time flow

1. Land on kdow.space, "Start free trial"
2. Sign up (email + password or Google)
3. Pick industry (Accounting / Real Estate / Content / Consulting / Other)
4. Workspace created with **4 pre-configured agents** for that industry
5. Sample tasks already in the inbox; user can assign real ones immediately

### Creating a task

```
"Research the top 5 competitors in accounting SaaS and compare pricing"
```

A Haiku router classifies → assigns to the right agent → task lands in
INBOX, moves to WORKING when picked up. Task creation also via voice
(push-to-talk) or industry templates.

### Task execution

1. Router (Haiku) classifies task → picks agent
2. Agent's industry-specific system prompt loads
3. Agent gets: task description + workspace memory + recent context
4. Sonnet runs with tools: `web_search`, `file_read`, `memory_read`,
   `memory_write`
5. Agent streams thinking/progress over WebSocket
6. Done → moves to REVIEW with deliverable attached
7. User: Approve (→ DONE), Edit (inline), or Redo (→ WORKING)

### The four agents per workspace (industry-keyed)

| Industry         | Agent 1       | Agent 2       | Agent 3       | Agent 4         |
| ---------------- | ------------- | ------------- | ------------- | --------------- |
| Accounting       | DataBot       | ClientBot     | ResearchBot   | ReportBot       |
| Real Estate      | ListingBot    | LeadBot       | ResearchBot   | TransactionBot  |
| Content/Marketing| WriterBot     | ResearchBot   | EditorBot     | StrategyBot     |
| Consulting       | ProposalBot   | ResearchBot   | ClientBot     | DeliveryBot     |
| Other (generic)  | Scout         | Builder       | Reviewer      | Planner         |

System prompts are stored as constants in the Rust codebase; on workspace
creation, four `agents` rows are seeded with industry-appropriate prompts,
roles, and avatar colors.

### Memory — the sticky feature

Every workspace accumulates persistent memory:

- "Client X prefers formal tone in emails"
- "Always include disclaimer in financial reports"
- "Use metric units, not imperial"
- "Brand colors: #1B2A4A, #C8A951"

Displayed in a sidebar ("Your workspace brain"). Users add/edit/delete
manually; agents reference relevant memories in every task; agents can
propose new memories from completed tasks for user confirmation.

### Voice — the wow feature

Push-to-talk via spacebar (desktop) or mic tap (mobile). Web Speech API
does STT in the browser. The transcript becomes a task or message. Agents
respond as text — TTS is a later addition. Voice is an input method, not
a product feature.

## Tech stack (pinned)

**Frontend**

- Next.js 15 (App Router, server components where possible)
- TypeScript (strict)
- TailwindCSS 4
- Zustand for client state
- next-auth (email + password + Google OAuth)
- Framer Motion for agent activity animations
- Lucide icons

**Backend (Rust)**

- Axum 0.8+
- SQLx with SQLite (embedded, WAL mode)
- reqwest for Anthropic / OpenAI calls
- tokio async runtime
- tower-http (CORS, compression, trace)
- serde + serde_json
- uuid for IDs
- argon2 for password hashing
- jsonwebtoken for JWT

**Payments**

- LemonSqueezy (already set up)
- Plans: Starter $25/mo, Pro $49/mo, Team $99/mo
- 7-day free trial on all plans

**Deployment**

- Frontend: Vercel
- Backend: Fly.io (single region to start, persistent volume for SQLite)
- Domain: `kdow.space` → frontend, `api.kdow.space` → backend

**Hard exclusions**: Prisma, Drizzle, tRPC, GraphQL, Redis, Postgres,
Docker (beyond Fly), Kubernetes, Supabase, Firebase, Electron, Tauri.

## Database schema (SQLite)

```sql
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    name TEXT,
    password_hash TEXT,
    google_id TEXT,
    industry TEXT,                          -- accounting|real_estate|content|consulting|other
    plan TEXT DEFAULT 'trial',              -- trial|starter|pro|team
    subscription_id TEXT,                   -- LemonSqueezy subscription ID
    subscription_status TEXT DEFAULT 'trialing',
    trial_ends_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE workspaces (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    industry TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE agents (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL,                     -- DataBot, Scout, …
    role TEXT NOT NULL,
    system_prompt TEXT NOT NULL,
    avatar_color TEXT NOT NULL,             -- hex for UI
    model TEXT DEFAULT 'claude-sonnet-4-6',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    project_id TEXT REFERENCES projects(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'inbox',            -- inbox|working|review|done|failed
    assigned_agent_id TEXT REFERENCES agents(id),
    priority TEXT DEFAULT 'normal',         -- low|normal|high|urgent
    deliverable TEXT,                       -- markdown output
    tokens_used INTEGER DEFAULT 0,
    cost_usd REAL DEFAULT 0,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    role TEXT NOT NULL,                     -- user|agent|system
    agent_id TEXT REFERENCES agents(id),
    content TEXT NOT NULL,
    tokens_in INTEGER,
    tokens_out INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    content TEXT NOT NULL,
    category TEXT,                          -- preference|fact|style|process
    source TEXT,                            -- manual|learned_from_task_<id>
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE usage (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    date TEXT NOT NULL,                     -- YYYY-MM-DD
    tasks_created INTEGER DEFAULT 0,
    messages_sent INTEGER DEFAULT 0,
    tokens_in INTEGER DEFAULT 0,
    tokens_out INTEGER DEFAULT 0,
    cost_usd REAL DEFAULT 0,
    UNIQUE(user_id, date)
);

CREATE TABLE templates (
    id TEXT PRIMARY KEY,
    industry TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    agent_role TEXT NOT NULL,
    is_global INTEGER DEFAULT 1
);
```

## Plans + privacy pivot

Privacy is the upsell, not a feature buried in settings. Accountants
handling client tax data will pay $99/mo for "nothing leaves your
device" — that's cheaper than one hour of their billable time.

| Plan       | Price   | Tasks/mo               | AI Mode                    | Privacy positioning              |
| ---------- | ------- | ---------------------- | -------------------------- | -------------------------------- |
| Starter    | $25/mo  | 100                    | Cloud only                 | "Never trains models"            |
| Pro        | $49/mo  | 500                    | Cloud + encrypted storage  | "End-to-end encrypted"           |
| Enterprise | $99/mo  | unlimited (cap 2000)   | Cloud + local + encrypted  | "Nothing leaves your device"     |

Cost per task (retail): 2 Sonnet calls (~8K in / ~3K out) + 1 Haiku call
(~2K in / ~500 out) = **~$0.07/task**.

| Plan       | Cloud cost (worst case) | Margin |
| ---------- | ----------------------- | ------ |
| Starter    | ~$0.70/mo               | 97%    |
| Pro        | ~$3.50/mo               | 93%    |
| Enterprise | ~$14/mo                 | 86%    |

Local-mode tasks (Enterprise) cost $0 in API spend — they boost margin
further on top of the table above.

### Model routing rules (cost discipline)

- Routing/classification → **Haiku** ($0.80/M in, $4/M out)
- Simple drafts (emails, formatting) → Haiku
- Research → **Sonnet** ($3/M in, $15/M out)
- Complex analysis → Sonnet
- Memory extraction → Haiku

### Plan gating

- Trial: 20 tasks total, 7 days
- 80% of plan limit → warning banner
- 100% → block new tasks, show upgrade prompt
- Team plan, >3000 tasks/mo → flag for review

## Security (non-negotiable for finance/accounting users)

1. HTTPS everywhere (Vercel + Fly enforce)
2. argon2 for passwords (never bcrypt, never SHA)
3. JWT 24h expiry, rolling refresh on activity
4. API keys in env vars, never DB or code
5. LLM payloads strip passwords / payment info
6. SQLite on encrypted Fly volume
7. Rate limit 60 req/min per user (tower middleware)
8. Input sanitization before LLM calls
9. CORS restricted to `kdow.space`
10. Privacy page: "Your data is never used to train AI models. Stored
    only in your workspace database."

"Bank-grade security" badge on landing with these specifics listed.

## LemonSqueezy integration

Three products in the Lemon dashboard:

- kdow Starter: $25/mo
- kdow Pro: $49/mo
- kdow Enterprise: $99/mo

7-day trial enabled on all three. Pricing page links straight to the
Lemon hosted checkout URLs. Webhook URL: `https://api.kdow.space/api/webhooks/lemonsqueezy`.

Events to handle (verify HMAC signature via `X-Signature` header — never
skip):

- `subscription_created` → set user `plan` + `subscription_id`
- `subscription_updated` → update plan
- `subscription_cancelled` → downgrade to free, retain data 30 days
- `subscription_payment_failed` → mark `past_due`
- `subscription_resumed` → restore plan

## Implementation order (4 weeks to beta)

### Phase 1 — Foundation (Week 1)

- Day 1–2: Rust backend skeleton (Axum + SQLite + migrations + health)
- Day 3–4: Auth (register, login, JWT, argon2, middleware)
- Day 5–6: Workspace + agent seeding (industry-keyed system prompts)
- Day 7: Next.js skeleton, landing, auth pages, onboarding

### Phase 2 — Task Board (Week 2)

- Day 8–9: Task CRUD + Haiku-based routing to agents
- Day 10–11: Anthropic streaming, model router (Haiku/Sonnet), token + cost tracking
- Day 12–13: WebSocket streaming to frontend
- Day 14: Kanban frontend + task detail panel

### Phase 3 — Polish + Payments (Week 3)

- Day 15–16: Memory system (CRUD + auto-injection + agent-proposed memories)
- Day 16.5–17.5 (**Phase 3.5 — Client-side encryption, 2 days**)
  - WebCrypto API key derivation from user passphrase (PBKDF2 → AES-GCM)
  - Encrypt conversation history + deliverables in the browser before
    POSTing to the server
  - Server stores ciphertext only for encrypted conversations
  - Decrypt in browser on load
  - "Privacy mode" toggle in settings (per-workspace)
  - LLM calls still go through the API in plaintext at request time
    (Anthropic/OpenAI cannot run on ciphertext); explain this clearly
    in the UI: "Your data is encrypted at rest. AI processing requires
    plaintext at the moment of inference."
  - Privacy badge on every workspace header: **Encrypted** or **Standard**
  - Pro plan unlock; Starter shows upgrade prompt when toggling on
- Day 18: LemonSqueezy webhook + plan gating
- Day 19: Voice input + industry templates
- Day 20: Mobile responsive
- Day 21: Deploy (Fly.io + Vercel + DNS + smoke test)

### Phase 4 — Beta Launch (Week 4)

- Day 22–23: Landing polish, demo GIF, pricing table
- Day 24–25: 20 beta users, monitor cost/errors
- Day 26–28: IndieHackers + Reddit + Twitter + Product Hunt prep

## Post-MVP — Local AI mode (Phase 6, Enterprise-only)

Premium feature gated to the $99 Enterprise tier. Justifies the price
gap above Pro for any user handling regulated data (tax records,
medical, legal, finance):

- **WebLLM** integration — Llama 3.1 8B or Phi-4 Mini, runs in-browser via WebGPU
- Model download on first use (~4GB, cached in browser via OPFS/IndexedDB)
- Local inference for simple tasks (drafting, formatting, summarization)
- Hybrid routing: local for simple, cloud (still encrypted) for complex
- IndexedDB for fully local conversation storage (Enterprise can opt out
  of any server-side persistence — server only sees auth + billing)
- "Fully Local" privacy badge appears when zero cloud calls are made for a task
- The pitch: **"Nothing leaves your device."** Cheaper than one billable hour.

## What is NOT in v1

- File upload (v2)
- MCP integrations: GitHub, Notion, Drive (v2 — core works first)
- Custom user-built agents (v2)
- Team/collab features (v2)
- Admin dashboard (v2)
- Email notifications + weekly digest (v2)
- Native mobile app (responsive web is enough)
- SSO / SAML (v3, enterprise)
- WebLLM / local AI mode (Phase 6 — Enterprise-only post-MVP)

## File structure

```
web/kdow/
├── backend/                    # Rust Axum
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── config.rs
│   │   ├── error.rs
│   │   ├── db/
│   │   │   ├── mod.rs
│   │   │   ├── migrations.rs
│   │   │   └── models.rs
│   │   ├── auth/
│   │   │   ├── mod.rs
│   │   │   ├── handlers.rs
│   │   │   └── middleware.rs
│   │   ├── api/
│   │   │   ├── mod.rs
│   │   │   ├── workspaces.rs
│   │   │   ├── tasks.rs
│   │   │   ├── agents.rs
│   │   │   ├── memories.rs
│   │   │   └── webhooks.rs
│   │   ├── llm/
│   │   │   ├── mod.rs
│   │   │   ├── anthropic.rs
│   │   │   ├── router.rs       # Haiku for routing, Sonnet for work
│   │   │   └── prompts.rs      # Industry-specific system prompts
│   │   ├── ws/
│   │   │   └── mod.rs          # WebSocket handler
│   │   └── templates/
│   │       └── mod.rs          # Task templates per industry
│   ├── fly.toml
│   └── Dockerfile
└── frontend/                   # Next.js
    ├── package.json
    ├── next.config.ts
    ├── tailwind.config.ts
    ├── src/
    │   ├── app/
    │   │   ├── page.tsx                        # Landing
    │   │   ├── layout.tsx
    │   │   ├── (auth)/{login,register}/page.tsx
    │   │   ├── onboarding/page.tsx
    │   │   └── workspace/
    │   │       ├── layout.tsx
    │   │       ├── page.tsx                    # Kanban
    │   │       ├── task/[id]/page.tsx
    │   │       ├── memory/page.tsx
    │   │       ├── templates/page.tsx
    │   │       └── settings/page.tsx
    │   ├── components/
    │   │   ├── task-board.tsx
    │   │   ├── task-card.tsx
    │   │   ├── task-detail.tsx
    │   │   ├── agent-avatar.tsx
    │   │   ├── voice-input.tsx
    │   │   ├── streaming-text.tsx
    │   │   └── memory-sidebar.tsx
    │   ├── lib/{api,ws,store}.ts
    │   └── types/index.ts
    └── vercel.json
```

## STOP-AND-ASK protocol (in force)

1. STOP on uncertainty about any external library, API, or webhook shape.
2. Paste the exact error or confusion.
3. State what was tried.
4. Wait for the fix.

Never guess Anthropic API parameters. Never guess LemonSqueezy webhook
shapes. Never guess MCP server configs. Pin what is pinned.

## Verification gate (every phase)

```bash
# Backend
cd web/kdow/backend && cargo build --release
cargo clippy -- -D warnings
cargo test

# Frontend
cd web/kdow/frontend && npm run build && npm run lint

# Integration
curl https://api.kdow.space/health           # 200
# Register → login → create workspace → verify 4 agents seeded
# Create task → verify routing → verify streaming
# Subscribe via LemonSqueezy test mode → verify webhook updates plan
```
