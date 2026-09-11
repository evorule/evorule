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

# 哥德尔、不自证与逼近真相：EvoRule 对"正确"的诚实立场

> **立场篇 · 承接《有所得，必有所失》《智能时代，选择不智能》**
> 前两篇讲了"敢做减法"和"选择不智能"；这一篇回答一个所有严肃系统都绕不开的问题——
> **你凭什么说你是对的？**
> EvoRule 的答案是：我们不声称绝对正确。我们只承诺，在现有技术条件下，最大化逼近事实真相。

---

## 一、每个系统都会被问到的那个问题

做一个系统，早晚会被问到同一个问题：

> "你的系统绝对正确吗？""能保证不出错吗？"

多数系统含糊其辞，或者干脆拍胸脯："我们经过严格验证，绝对可靠。"

EvoRule 的回答有些不同，甚至有点"扫兴"：

> **"我们不声称绝对正确。"**

这不是谦虚，更不是推卸——这是一个经过认真思考后的立场。

---

## 二、哥德尔不完备定理：为什么"绝对自证"是个伪命题

1931 年，数学家哥德尔证明了两个让整个数学界震动的定理，其中第一条大意是：

> 任何足够强大的公理系统，都必然存在**系统自身内部无法证明，也无法证伪**的命题。

也就是说：一个系统想用"我自己检查过了，所以我是对的"来证明自己的正确，**在原理上就做不到**。总有一些"真相"，系统用自己内部的语言永远够不着。

这对工程世界是一记警钟：

> **任何声称"绝对正确 / 绝对安全 / 绝对无 bug"的系统，要么在回避自己的边界，要么根本没意识到自己有边界。**

"绝对"，不是一个可被证明的状态，而更像一句不可证伪的口号。

---

## 三、EvoRule 的立场：不声称绝对，只逼近真相

既然"绝对自证"在原理上不可得，EvoRule 选择换一条路——**把"正确"从一句口号，变成一个持续逼近的过程**：

> **不声称绝对正确；但在现有技术条件下，尽最大努力，最大化逼近事实真相。**

- **不声称绝对** —— 承认边界，不吹嘘"全对"。
- **最大化逼近** —— 用一切可检验的手段，把"不知道对不对"的区间，压到技术条件下能压到的最小。

我们不承诺一个"不可能被证明"的完美，而是承诺一种"可以持续改进"的逼近。

---

## 四、怎么"逼近"：EvoRule 的工程手段

逼近不是空话。EvoRule 用一套可检验的手段，不断拉近自己与真相的距离：

- **确定性执行** —— 同一输入，同一输出。行为可复现，拒绝"玄学"。
- **可回放、可审计** —— 每一步执行都留痕；出了错，能精确回放到源头，而不是一句"模型抽风了"。
- **形式化验证** —— 用 Kani 验证关键路径、用 TLA+ 做模型检验，在抽象层面对状态机做数学层面的推演。
- **大规模测试** —— 664 个测试覆盖行为契约，把预期固化成可回归的断言。
- **最小攻击面** —— 零 unsafe、机制层纯净，让"可能出错的地方"本身就更少。

注意：以上每一项，**都不是"证明绝对"**，而是在**收窄不确定性**。我们清楚这些手段各自的极限——这正是哥德尔教给我们的清醒。

---

## 五、诚实即可信

这恰恰是 EvoRule 反直觉、却更值得信任的地方：

> 一个承认"我有边界"的系统，比一个声称"我全对"的系统更可靠。

因为前者说的每一句"我能保证"，你都愿意相信；后者说的一切，你都要先打个折扣。

在推广语境里，这看起来像"示弱"，实际上是最强的信任资产——

- 我们不把测试数吹成"绝对安全"，而是说"这是我们逼近真相的证据"。
- 我们不把验证结果包装成"正确性证明"，而是说"这是我们当前技术条件下所能达到的最接近真相的度量"。

**信任，不来自宣称完美，来自如实交代边界之后，依然交付了可验证的逼近。**

---

## 六、收尾

哥德尔告诉我们一个有点"泄气"、又无比清醒的事实：

> 足够强大的系统，无法完全自证自己。

但它的启示不是"所以没法做对"，而是——

> **"所以别假装自己已经做到绝对对。"**

EvoRule 选择诚实：不声称绝对正确，但在现有技术条件下，最大化逼近事实真相。

它不把"绝对"当终点去吹嘘，而是把"逼近"当过程去践行。

> **我们无法自证完美，但我们能证明自己在逼近。**

---

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