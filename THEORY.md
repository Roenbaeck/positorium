# The Philosophical Foundations of Positorium

## What Needs to Be Agreed Upon

To communicate efficiently and intelligibly, we must first agree on a few fundamental concepts. Imagine transcribing a discussion between two individuals. For a third, non-participating individual to unambiguously interpret the transcription, certain agreements are necessary.

Let’s say one person frequently talks about ‘Jennifer’, while the other uses ‘Jen’. To avoid confusion, it must be agreed that ‘Jen’ and ‘Jennifer’ refer to the same person. This can be achieved by introducing a unique identifier, say ‘J42’. The transcript might then read, “I talked to Jen (J42) today,” and “Oh, how is Jennifer (J42) doing?”. We must agree on the **identities** of the things we discuss.

If the response is, “Jen (J42) is doing good,” followed by, “I figured as much, new boyfriend and all, so Jennifer (J42) must be good,” the term ‘good’ is a **value** that should mean the same thing to both individuals. While ‘good’ is not scientifically precise, a consensus and familiarity with the value allow for mutual understanding. Any imprecisions that could lead to confusion can be sorted out in the discussion itself. If the statement was, “Jen (J42) is doing so-so,” a natural response would be, “What do you mean ‘so-so’? Is something the matter?”. We must agree on values, accepting that any imprecisions lie within a tolerable margin of error for mutual understanding.

Now, if we transcribe this efficiently, leaving out the nuances of natural language, we can test whether these constructs capture the essence of the discussion. Using identities and values, we can form pairs like `(J42, "Jen")`, `(J42, "Jennifer")`, and `(J42, "good")`. However, something is missing. There is no way to tell what ‘Jen’, ‘Jennifer’, and ‘good’ are with respect to the identity J42. This can be resolved by adding the notion of a **role** that an identity plays. The pairs then become triples: `(J42, nickname, "Jen")`, `(J42, first name, "Jennifer")`, and `(J42, mood, "good")`. We must agree on the roles that identities take on.

Is this enough? Not quite. Unless Jen is permanently in a good mood, the triple `(J42, mood, "good")` is not temporally determined. A third party reading the transcript years later wouldn't know when it applied. Using a **temporal determinator**, interpretable as ‘since when’, the triples become quadruples: `(J42, nickname, "Jen", '1988')`, `(J42, first name, "Jennifer", '1980-02-13')`, and `(J42, mood, "good", '2019-08-20T09:45:00')`. The precision of time can vary, reflecting the inherent imprecision in how we measure and recall it. We must agree on the points in time when facts hold, to some degree of precision.

This is almost all we need, but it only expresses properties of an individual. What about relationships? Let B43 be the identity of Jen’s boyfriend. A quadruple like `(J42, boyfriend, B43, '2019')` is problematic. First, B43 is an identity, not a value. Second, it’s ambiguous: is B43 the boyfriend of J42, or is J42 the boyfriend of B43? Finally, relationships can involve more than two identities.

The solution is to use a structure where the first position contains a set of `(identity, role)` pairs. This resolves the ambiguity. For example: `[{(J42, girlfriend), (B43, boyfriend)}, "official", '2019']`. The second position is a value, and the third is the temporal determinator. We can consolidate properties into this format as well: `[{(J42, nickname)}, "Jen", '1988']`. The only difference is the number of pairs in the set. In Transitional Modeling, these structures are called **posits**.

## What Can Be Disagreed Upon

Even if you understand what a posit is saying, it doesn’t mean you believe it. Many different opinions can be held about a single statement. To talk about posits themselves, we must give them identities. Let’s say `P1` is the identity for `[{(J42, girlfriend), (B43, boyfriend)}, "official", '2019']` and `P2` is for `[{(J42, nickname)}, "Jen", '1988']`. These identities allow us to create meta-posits.

To avoid confusion, we reserve specific roles for this purpose, such as `posit` and `ascertains`. An **assertion** is a meta-posit, exemplified by `[{(P1, posit), (J42, ascertains)}, 80%, '2019-04-05']`. This means Jennifer (J42) expresses an 80% confidence that her relationship with B43 was official since 2019. In contrast, `[{(P1, posit), (B43, ascertains)}, -100%, '2019-04-05']` reveals a conflict.

Certainty values fall within the `[-100%, 100%]` interval. A positive value indicates belief in the stated fact, while a negative value indicates belief in its opposite. A certainty of `-100%` means complete certainty in the contrary. For example, the boyfriend is completely certain of the posit `[{(J42, girlfriend), (B43, boyfriend)}, "anything but official", '2019']`. A certainty of `0%` signifies complete uncertainty.

This introduces a powerful asymmetry. While you can be 100% certain of only a single posit for a given appearance set at a specific time, you can be -100% certain of an infinite number of posits without contradicting yourself. Being -100% certain that the value is "official" is not a contradiction to being -100% certain that the value is "a secret", as both simply affirm that the value is *something else*. For certainties between the extremes, it becomes computationally possible to determine whether a collection of opinions is logically consistent or contradictory.

## What Will Change and What Will Remain

Change is everywhere. Values change, and opinions change. A living transcript must capture these changes non-destructively. Anything written is written in stone.

Jennifer broke up with her boyfriend. The original posit, `P1`, was `[{(J42, girlfriend), (B43, boyfriend)}, "official", '2019']`. A new posit, `P3`, tells us what happened in 2020: `[{(J42, girlfriend), (B43, boyfriend)}, "broken up", '2020']`. `P1` and `P3` are different posits, but they share something that `P2` (`[{(J42, nickname)}, "Jen", '1988']`) does not.

Change can be defined precisely: when two posits share the same set of `(identity, role)` pairs—the **appearance set**—but have different values and one time point follows the other, they describe a change. Thus, `P3` is a change from `P1`. The appearance set remains indefinitely, while its associated values may change entirely.

This applies to assertions as well. Jennifer’s certainty in `P1` might change after learning of her boyfriend’s opinion. A new assertion, `[{(P1, posit), (J42, ascertains)}, 0%, '2019-09-21']`, would capture her revised certainty level.

This mechanism is similar to a ‘logical delete’ in a bitemporal database, where certainty is limited to `1` (recorded) or `0` (deleted), and only the database itself can have an opinion. Transitional Modeling extends this concept, approaching probabilistic databases when certainties are in the `[0, 100%]` range and uncertainty theory when they are in the `[-100%, 100%]` range. The ability for anyone to have an opinion is similar to multi-tenant databases.

Now, what if we find another posit, `P4`, as `[{(S44, nickname)}, "Jen", '1988']`? Were there two Jens all along?

## What We Are

The transcript is not yet exhaustive. We have presumed that J42 is a female human being, but presumptions lead to headaches. We need to solve the mystery of identity S44.

An unrelated utterance in the discussion reveals the answer: “Haha, but I wonder what Jen (J42) feels about being hit by Jen (S44)? That storm is about to hit shores tomorrow.” There is a person, J42, and a storm, S44, both nicknamed Jen.

To define what things are, we can reserve roles like `thing` and `class`. We can then create posits to classify our identities: `[{(J42, thing), (C1, class)}, "active", '1980-02-13']` and `[{(S44, thing), (C2, class)}, "active", '2019-08-10']`. The classes themselves can be described with more posits, such as `[{(C1, named)}, "Female Child", '2019-08-20']` and `[{(C2, named)}, "Storm", '2019-08-20']`.

Things can change class over time. If Jennifer (J42) becomes an adult at 18, we can add `[{(J42, thing), (C3, class)}, "active", '1998-02-13']`, where `C3` is the class for "Female Adult". This is another example of change, as the appearance set is the same, but the value (the class) and time are different.

Different observers can also have different models. A third party might prefer a more generic classification, like `[{(J42, thing), (C4, class)}, "active", '1980-02-13']`, where `C4` is "Person". These concurrent models can coexist. Furthermore, classes can be related hierarchically, for example: `[{(C1, subclass), (C4, superclass)}, "active", '2019-08-20']`.

## Rethinking the Database

Transitional Modeling provides a theoretical framework for representing the subjectivity, uncertainty, and temporality of information. Traditional databases, particularly SQL databases, demand high conformance. Data must conform to tables, keys, data types, constraints, and a single version of the truth. Non-conforming data is often discarded, forced into a generic type like JSON, or molded through complex logic until it fits.

NoSQL databases flourished by offering near-zero conformance, allowing users to dump any data they wished. However, this simply shifted the burden of ensuring consistency to every read operation.

The way forward lies between these two extremes. A transitional database aims to minimize conformance requirements on write while still providing the mechanics for schemas, constraints, and classifications. These constructs are subjective, evolving, and can be applied late, leading to a concept of “eventual conformance.”

With a transitional database built on posits, we can run a wide variety of queries:
-   **NVP-like search**: Search anywhere for a unique identifier.
-   **Graph-like search**: Search for everything that has a specific role or every time an identity played a role.
-   **Relational-like search**: Search for everything with a certain property or all instances of a class.
-   **Hierarchical-like search**: Search for all subclasses of a given class.
-   **Temporal-like search**: Search as it was on a given date.
-   **Bi-Temporal-like search**: Search given what we knew on a given date.
-   **Multi-tenant-like search**: Search for disagreements between identities.
-   **Probabilistic-like search**: Search for information that is at least 75% certain.
-   **Audit-like search**: Search for corrections made between two dates.
-   **Log-like search**: Search for all model changes made by a specific identity.

It can also answer novel questions:
-   How many times has consensus been reached?
-   How many times have opposite opinions been expressed?
-   Which individuals have contradicted themselves?
-   When was a particular constraint in place?

Positorium is the realization of such a database, written in Rust, bringing the power of Transitional Modeling to life.
