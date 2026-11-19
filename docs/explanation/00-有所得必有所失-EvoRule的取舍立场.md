<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published
  by the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

<a id="english"></a>

# No Gain Without Loss: EvoRule's Position on Trade-offs

> **Position statement · the first piece of the adoption narrative**
> This piece makes no feature claims and no "here is what it can do" promises. It answers a more fundamental question first: **what gives EvoRule the nerve to subtract?**
> What comes after it is the technical argument ("Why an Execution Engine That Only Accepts JSON") and the hands-on tutorial (Quick Start).

---

## 1. Admitting That There Is No Free Lunch

In the world of engineering I have seen too many "give you everything" systems: general-purpose and high-performance and easy to adopt and auditable and distributed and AI-native, all at once. The result is usually — **a little bit of everything, and not enough of anything.**

I have come to believe in a plain old saying more and more: **no gain without loss.**

You cannot have determinism and unlimited expressive freedom at the same time; you cannot be transparent and auditable while hiding implementation shortcuts; you cannot be pure and restrained while being all things to all people.

**EvoRule's position is to face this arithmetic of gain and loss head-on — and to lay the answer out on the table.**

---

## 2. What EvoRule Chooses, and What It Gives Up

I have written EvoRule's trade-offs down as an honest ledger:

| What I gain | What I give up in exchange |
|---|---|
| **Deterministic** — the same input always produces the same output | **Expressiveness** — no Lambda, no complex type inference; JSON is deliberately "dumb" |
| **Auditable** — every step has a causal chain; the ledger is replayable and verifiable | **Convenient shortcuts** — no hidden DSL, no logic "tucked away" |
| **Transparent** — rules, state, and events are all JSON, directly readable and writable | **Abstraction tricks** — no bespoke syntax invented to be incomprehensible |
| **Pure** — a minimized mechanism layer, zero unsafe, three strictly layered tiers | **Omnipotence** — not a general-purpose rule engine, not a workflow platform, not a distributed database |
| **Provable** — Kani / TLA+ pin the invariants down | **Shortcuts** — for many things, "doing it again properly" is worth more than "cutting corners once" |
| **Restrained** — execution only; no LLM, no memory, no planning | **Hype** — no riding the "AI framework" wave |

> In one sentence: **I traded "functional poverty" for "abundance of trust."**

---

## 3. Why EvoRule Dares to Subtract

Some will ask: **if you cut this much away on purpose, aren't you crippling yourself?**

My answer sits in the line the README keeps repeating: **"This is a boundary, not a bug."**

- Limited JSON expressiveness — **a boundary, not a bug**. It is exactly the price of transparency and auditability, and it is deliberate.
- Not a general-purpose rule engine — **a boundary, not a bug**. The iron rule of "JSON only" is itself the source of differentiation.
- Not an AI agent framework — **a boundary, not a bug**. EvoRule's value is "giving the LLM a trustworthy execution layer," not doing for you what the LLM should do.

**"What it is not" defines "what it is."** A project is at its clearest not when it announces what it can do, but when it honestly admits what it refuses to do.

Restraint is precisely where trust comes from. Only a system that dares to say "I don't do this" is one you dare to entrust with anything that matters.

---

## 4. What This Trade-off Means for You

Choose EvoRule, and you will —

**Gain:**
- a rule you can **read and understand yourself** (JSON, nothing hidden)
- an audit ledger that is **replayable, comparable, and verifiable**
- a deterministic kernel that is **formally provable**
- a clean boundary that **decouples business logic from engineering**

**Give up:**
- flashy ways to express things (no Lambda; accept the plainness of JSON)
- the temptation of "just a bit faster" (determinism outranks peak performance)
- the fantasy of "it can do anything" (it does exactly one thing — execution — and does it thoroughly)

It is **not for** people who want "one library to solve every problem." It **is for** those willing to pay a small price for **explainability, auditability, and trust** — compliance, audit, deterministic workflows, and every scenario that needs to land LLM output on a reliable execution layer.

---

## 5. Closing

The README opens with:

> _Rules do not speak. They only run. And we are among the first witnesses._

EvoRule's position is a belief that **"less is trustworthy"**: only by daring to lose do you deserve what is most worth having.

> **No gain without loss.**
> **EvoRule gives up "all-capable" to gain "trustworthy."**
>
> — No intelligence, only execution. Deterministic execution, traceable, auditable.