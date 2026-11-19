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

# Gödel, Non-Self-Certification, and Approaching the Truth: EvoRule's Honest Position on "Correctness"

> **Position piece · follows "No Gain Without Loss" and "In the Age of Intelligence, Choose Not to Be Intelligent"**
> The first two pieces covered "daring to subtract" and "choosing not to be intelligent"; this one answers the question no serious system can dodge —
> **what entitles you to say you are right?**
> EvoRule's answer: we do not claim absolute correctness. We only promise that, with the technology available today, we maximize how closely we approach the facts.

---

## 1. The Question Every System Gets Asked

Build a system, and sooner or later you will be asked the same question:

> "Is your system absolutely correct?" "Can you guarantee it will never fail?"

Most systems mumble, or pound their chest outright: "We have been rigorously verified. Absolutely reliable."

EvoRule's answer is a little different, even a bit of a party-pooper:

> **"We do not claim to be absolutely correct."**

This is not modesty, and it is certainly not evasion — it is a position reached through serious thought.

---

## 2. Gödel's Incompleteness Theorems: Why "Absolute Self-Certification" Is a Pseudo-Question

In 1931 the mathematician Kurt Gödel proved two theorems that shook the entire mathematical world. The first says, roughly:

> Any axiom system powerful enough will necessarily contain propositions that **can neither be proved nor disproved from within the system itself**.

In other words: a system that tries to prove its own correctness by saying "I checked myself, therefore I am right" **cannot succeed in principle**. There will always be truths the system can never reach with its own internal language.

For the world of engineering this is an alarm bell:

> **Any system that claims to be "absolutely correct / absolutely secure / absolutely bug-free" is either dodging its own boundaries, or has never realized that it has any.**

"Absolute" is not a provable state; it is closer to an unfalsifiable slogan.

---

## 3. EvoRule's Position: Claim No Absolutes, Approach the Truth

Since "absolute self-certification" is unattainable in principle, EvoRule takes a different road — **turning "correctness" from a slogan into a process of continual approximation**:

> **We claim no absolute correctness; but with the technology available today, we do our utmost to approach the truth as closely as possible.**

- **Claim no absolutes** — acknowledge the boundary; never brag about being "all correct."
- **Maximize the approach** — use every testable means to shrink the zone of "we don't know whether this is right" down to the smallest that today's technology allows.

We do not promise a perfection that "cannot be proved." We promise an approximation that "can keep improving."

---

## 4. How the Approach Works: EvoRule's Engineering Means

The approach is not empty talk. EvoRule uses a set of testable means to keep closing the distance between itself and the truth:

- **Deterministic execution** — same input, same output. Behavior is reproducible; no "mysticism" allowed.
- **Replayable and auditable** — every step of execution leaves a trace; when something goes wrong, you can replay it back to the source instead of waving it off as "the model glitched."
- **Formal verification** — Kani verifies the critical paths and TLA+ runs model checking, reasoning about the state machine mathematically at the abstract level.
- **Large-scale testing** — 664 tests cover the behavioral contracts, freezing expectations into regression assertions.
- **Minimal attack surface** — zero unsafe and a pure mechanism layer mean there are fewer places where things can go wrong in the first place.

Note: none of the items above **proves anything absolute**. Each one **narrows uncertainty**. We know the limits of every one of these means — that is exactly the sober clarity Gödel taught us.

---

## 5. Honesty Is What Earns Trust

This is precisely where EvoRule is counter-intuitive — and more deserving of trust:

> A system that admits "I have boundaries" is more dependable than one that claims "I am entirely right."

Because every "I can guarantee this" from the former is one you are willing to believe; everything the latter says, you discount first.

In an adoption pitch this looks like "showing weakness." In reality it is the strongest trust asset there is —

- We do not inflate our test counts into "absolute safety"; we say, "this is our evidence of approaching the truth."
- We do not package verification results as "proof of correctness"; we say, "this is the closest measure to the truth that our current technology can reach."

**Trust comes not from claiming perfection, but from stating your boundaries honestly and still delivering a verifiable approximation.**

---

## 6. Closing

Gödel tells us a fact that is somewhat deflating, yet utterly sobering:

> A system powerful enough cannot fully certify itself.

But the lesson is not "so there is no way to be right." It is —

> **"So stop pretending you have already achieved absolute correctness."**

EvoRule chooses honesty: we claim no absolute correctness, but with the technology available today we maximize how closely we approach the truth.

It does not brag about "the absolute" as an endpoint; it practices "the approach" as an ongoing discipline.

> **We cannot certify our own perfection, but we can prove that we are approaching it.**