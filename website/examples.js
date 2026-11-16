// The build copies the seed data from the repository's tested welcome example.
const current = `search ?claim = [
  {(?company, status)}, ?assessment, *
] in effect '2026-01-12 10:00:00',
            '2026-01-12 10:00:00'
  via [{(?claim, posit), (?source, ascertains)},
       ?certainty, *],
  [{(?source, name)}, ?source_name, *]
return ?assessment, ?source_name, ?certainty;`;

export const examples = {
  current: {
    query: current,
    question: "What is in effect after the analyst’s correction?",
  },
  rewind: {
    query: current.replace("'2026-01-12 10:00:00',", "'2026-01-12 09:15:00',"),
    question: "What did the sources say before the analyst changed their mind?",
  },
  history: {
    query: `search ?claim = [
  {(?company, status)}, ?assessment, *
],
  [{(?claim, posit), (?source, ascertains)},
   ?certainty, ?asserted],
  [{(?source, name)}, ?source_name, *]
return ?assessment, ?source_name,
       ?certainty, ?asserted
order by ?asserted;`,
    question: "What is the complete trail, including the 0% retraction?",
  },
  plan: {
    query: `explain ${current}`,
    question:
      "How will Positorium execute this query? EXPLAIN returns the typed execution plan.",
  },
};
