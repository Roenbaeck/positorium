<img src="./Traqula.svg" alt="Traqula Language Reference" width="200">
<p/>

Welcome to the Traqula Study Guide! This guide is designed to be self-contained—no external files required. It will help you learn Traqula, Positorium's domain-specific language for managing and querying transitional data. We'll start with the basics and build up to advanced concepts, using plenty of working examples. By the end, you'll be able to write your own Traqula scripts.

---

## Cheat Sheet

- `add role <role1>, <role2>, ...;` — Declare roles
- `add posit [{(identity, role)}, value, time][, ...];` — Insert facts
- `search <pattern> [where <condition>] [return <projection>] [limit <N>];` — Query data
- `+var` — New identity (or insert matches into existing)
- `var` — Recall identity (no insert)
- `*` — Wildcard (match anything without keeping track of it)
- `as of <time>` — Snapshot reduction
- `where <condition>` — Filter results
- `return <vars>` — Output variables
- `limit <N>` — Cap results

---

## Glossary

- **Thing**: An opaque identity (person, posit, etc.)
- **Role**: A label (name, wife, age)
- **Appearance**: (Thing, Role) pair
- **AppearanceSet**: Set of appearances (the "viewport" for a posit)
- **Posit**: A proposition: (AppearanceSet, Value, Time)
- **Certainty**: Confidence in a posit (e.g., `75%`)
- **Time**: When a posit is asserted

---

## Lesson 1: Core Concepts – What is a Posit?

A **posit** is a statement like: "Alice's name was 'Alice Smith' starting on January 1, 2023."

- **AppearanceSet**: A set of (identity, role) pairs that define the "shape" or "viewport" of the posit. Think of it as a template with slots for specific roles, like holes in a piece of paper that you place over data to see what fits through.
  - For example, `{(Alice, name)}` is a viewport with one hole: the "name" role aimed at Alice. When you look through this viewport at the database, you might see the value "Alice Smith" at time '2023-01-01'.
  - For a couple: `{(Bob, husband), (Carol, wife)}` has two holes – one for the "husband" role (Bob) and one for the "wife" role (Carol). Aiming this at the data might reveal the value "married" at a certain time.
  - This "aiming" happens during searches or when adding posits, binding identities to roles to extract or assert facts.
- **Value**: The fact being asserted, like `"Alice Smith"`.
- **Time**: When this fact became true, like `'2023-01-01'`.

Posits don't replace old data; they add layers. If Alice changes her name, you add a new posit with a later time.

---

## Lesson 2: Getting Started – Running Traqula Scripts

To follow along, start the Positorium server (see README.md). Use the web console at `http://localhost:8080` or send scripts via API.

Scripts are sequences of commands separated by semicolons. Variables persist across commands.

Example script structure:
```
add role name, age;
add posit [{(+person, name)}, "Bob", '2020-01-01'];
search [{(*, name)}, +n, *] return n;
```

Run this in the console to see "Bob" returned.

---

## Lesson 3: Adding Data – Roles and Posits

### Adding Roles

Roles define what you can talk about. Declare them first.

Syntax: `add role <role1>, <role2>, ...;`

Example:
```
add role name, age, city;
```

This creates roles for names, ages, and cities.

### Adding Posits

Insert facts into the database.

Syntax: `add posit [AppearanceSet, Value, Time][, ...];`

- AppearanceSet: `{(identity, role), ...}`
- Use `+var` to create new identities (e.g., `+person`).
- Use `var` to reference existing ones.

Example: Create a person with a name.
```
add role name;
add posit [{(+alice, name)}, "Alice", '2023-01-01'];
```

This creates a new identity for Alice and posits her name.

Multiple posits in one command:
```
add posit [{(alice, name)}, "Alice Smith", '2023-06-01'],
          [{(alice, age)}, 30, '2023-01-01'];
```

Now Alice has a full name and age.

**Try it:** Add a role for "city" and posit Alice's city as "New York" starting '2023-01-01'.

---

## Lesson 4: Basic Searches – Finding Data

Search for posits using patterns.

Syntax: `search <pattern> [where <condition>] [return <projection>] [limit <N>];`

### Simple Pattern Matching

Pattern mirrors posit structure: `[{AppearanceSet}, Value, Time]`

Use `*` for wildcards, `+var` to bind variables.

Example: Find all names.
```
search [{(*, name)}, +n, *] return n;
```

This matches any posit with role "name", binds the value to `n`, and returns it.

Output: `n: "Alice"`

### Matching Specific Values

```
search [{(*, name)}, +n, "Alice", *] return n;
```

Finds posits where value is exactly "Alice", binding the value to `n` and returning it.

### Binding Identities

```
search [{(+person, name)}, +name_val, *] return person, name_val;
```

Binds the identity to `person` and value to `name_val`.

**Try it:** Search for Alice's age and return the value.

---

## Lesson 5: Variables and Binding – Reusing Identities

Variables make scripts powerful.

- `+var`: New identity.
- `var`: Existing identity.
- `*`: Wildcard (no binding).

Example: Create and reference.
```
add role wife, husband;
add posit [{(+alice, wife), (+bob, husband)}, "married", '2020-01-01'];
search [{(alice, wife), (bob, husband)}, +status, *] return status;
```

`alice` and `bob` persist, so you can use them in later commands.

### Unions in Patterns

Use `|` for "either role".

Example: Names of wives or husbands.
```
search [{(alice|bob, name)}, +n, *] return n;
```

Matches if Alice or Bob has a name.

---

## Lesson 6: Filtering with WHERE – Narrowing Results

Add conditions after `search`.

Supported: Time (`t`, `t1 == t2`), Value (`v == "text"`, `age > 25`), Certainty (`c >= 80%`).

Example: Names valid before 2000.
```
search [{(*, name)}, +n, +t] where t <= '1999-12-31' return n, t;
```

**Try it:** Find ages greater than 25.

---

## Lesson 7: Temporal Queries with 'as of' – Snapshots vs. History

`as of` reduces to the latest posit per appearance set at or before a time.

- Literal: `as of '2020-01-01'`
- Variable: `as of mt` (per binding)

Example: Current names (snapshot).
```
search [{(*, name)}, +n, *] as of @NOW return n;
```

Vs. history up to now:
```
search [{(*, name)}, +n, +t] where t <= @NOW return n, t;
```

The first returns one name per person (latest); the second returns all historical names.

Example: Names at marriage time.
```
add role wife, husband, name;
add posit [{(+w, wife), (+h, husband)}, "married", '2004-06-19'],
          [{(w, name)}, "Bella Trix", '1972-12-13'],
          [{(w, name)}, "Bella Bald", '2024-05-29'];
search [{(+w, wife), (+h, husband)}, "married", +mt] as of @NOW,
       [{(w|h, name)}, +n, +t] as of mt
return n, t, mt;
```

This finds current marriages, then names as of marriage time.

**Try it:** Modify to find names at divorce time.

---

## Lesson 8: Projections and Limits – Controlling Output

### RETURN

Specify what to output. 

Example: `return name, age;`

### LIMIT

Cap results: `limit 5;`

Includes a flag if more exist.

---

## Data Types and Literals

- Strings: `"text"`
- Numbers: `42` (int), `3.14` (decimal)
- JSON: `{"key": "value"}`
- Certainty: `75%`
- Time: `'2023-01-01'`, `@NOW`, `@BOT`, `@EOT`

---

## Full Example: Family and History

```
add role wife, husband, name, age;

add posit [{(+p1, wife), (+p2, husband)}, "married", '2004-06-19'],
          [{(p1, name)}, "Bella", '1972-02-13'],
          [{(p2, name)}, "Archie", '1972-08-20'];

search [{(+w, wife), (+h, husband)}, "married", *],
       [{(w|h, name)}, +n, *]
return n;

search [{(w, name)}, +n, +t] where t <= '2020-01-01' return n, t;

search [{(w, name)}, +n, *] as of '2020-01-01' return n;
```

---

## Troubleshooting

- **No results?** Check roles are added, times match, variables bound.
- **Too many results?** Add WHERE filters or LIMIT.
- **Syntax errors?** Patterns must match posit structure exactly.
- **Temporal confusion?** Remember: `as of` is snapshot (one per set), `where t <=` is history.

Experiment in the web console. Happy querying!