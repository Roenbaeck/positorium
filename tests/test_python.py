import tempfile
import unittest
from datetime import date, datetime, timezone
from decimal import Decimal
from pathlib import Path

import positorium


class PositoriumPythonTests(unittest.TestCase):
    def test_distribution_and_contract_versions_match(self):
        self.assertEqual(positorium.__version__, "0.1.4b2")
        self.assertEqual(positorium.RUST_PACKAGE_VERSION, "0.1.4-beta.2")
        self.assertEqual(positorium.PYTHON_INTERFACE_VERSION, 1)
        self.assertEqual(positorium.TRAQULA_VERSION, 1)
        self.assertEqual(positorium.TERRAIN_VERSION, 1)

    def test_memory_database_returns_lossless_structured_rows(self):
        with positorium.Database.memory() as database:
            result = database.execute(
                """
                add role name, score;
                add posit [{(+person, name)}, "Ada", '2024-01-01'],
                          [{(person, score)}, +0010.00, '2024-01-01'];
                search [{(?person, name)}, ?name, *],
                       [{(?person, score)}, ?score, *]
                return ?person, ?name, ?score;
                """,
                now=positorium.time("2025-01-01"),
            )

            self.assertEqual(result.python_interface_version, 1)
            self.assertEqual(result.traqula_version, 1)
            self.assertEqual(result.resolved_now, "2025-01-01")
            self.assertEqual(len(result), 1)
            row = result[0][0]
            self.assertEqual(row["person"].kind, "thing")
            self.assertEqual(row["name"].text, '"Ada"')
            self.assertEqual(row["score"].text, "+0010.00")
            self.assertEqual(result[0].to_dicts(text=True)[0]["score"], "+0010.00")

            database.execute(
                'add posit [{(+other, name)}, "Bob", \'2024-01-01\'], '
                "          [{(other, score)}, 20, '2024-01-01'];"
            )
            limited = database.execute_one(
                "search [{(?person, name)}, ?name, *] return ?person, ?name;",
                max_rows=1,
            )
            self.assertEqual(limited.row_count, 1)
            self.assertTrue(limited.limited)

        self.assertTrue(database.closed)
        with self.assertRaises(positorium.ClosedError):
            database.execute("add role never_added;")

    def test_typed_parameters_and_helpers(self):
        with positorium.Database.memory() as database:
            database.execute(
                "add role name, score; "
                "add posit [{(+person, name)}, \"Ada\", '2024-01-01'], "
                "          [{(person, score)}, +0010.00, '2024-01-01'];"
            )
            result = database.execute_one(
                "search [{(?person, name)}, $name, *], "
                "       [{(?person, score)}, ?score, *] as of $cutoff "
                "where ?score = $score return ?person, ?score;",
                parameters={
                    "name": positorium.literal("Ada"),
                    "score": positorium.raw_literal("10"),
                    "cutoff": positorium.time(date(2025, 1, 1)),
                },
            )
            self.assertEqual(result.row_count, 1)
            self.assertEqual(result[0]["score"].text, "+0010.00")

        self.assertEqual(positorium.literal(42).text, "42")
        self.assertEqual(positorium.literal(Decimal("10.00")).text, "10.00")
        self.assertEqual(positorium.Literal.certainty(-75).text, "-75%")
        self.assertEqual(positorium.literal({"name": "Ada"}).text, '{"name":"Ada"}')
        instant = datetime(2025, 1, 1, 2, 30, tzinfo=timezone.utc)
        self.assertEqual(positorium.time(instant).text, "'2025-01-01 02:30:00'")

    def test_multiple_results_and_execute_one_contract(self):
        with positorium.Database.memory() as database:
            result = database.execute(
                "add role name; "
                "add posit [{(+person, name)}, \"Ada\", @NOW]; "
                "search [{(?person, name)}, ?name, *] return ?name; "
                "search [{(?person, name)}, ?name, *] return ?person;"
            )
            self.assertEqual(len(result), 2)
            with self.assertRaisesRegex(ValueError, "exactly one"):
                database.execute_one(
                    "search [{(?person, name)}, ?name, *] return ?name; "
                    "search [{(?person, name)}, ?name, *] return ?person;"
                )

    def test_persistent_store_reopens_after_context_exit(self):
        with tempfile.TemporaryDirectory() as temporary:
            store = Path(temporary) / "python.store"
            with positorium.Database.open(store) as database:
                database.execute(
                    "add role name; add posit [{(+person, name)}, \"Ada\", '2024-01-01'];"
                )
                self.assertEqual(database.path, store)
                with self.assertRaises(positorium.PersistenceError):
                    positorium.Database.open(store)

            with positorium.Database.open(store) as reopened:
                result = reopened.execute_one(
                    "search [{(?person, name)}, ?name, *] return ?name;"
                )
                self.assertEqual(result[0]["name"].text, '"Ada"')

    def test_terrain_is_native_versioned_data(self):
        with positorium.Database.memory() as database:
            database.execute(
                "add role name; add posit [{(+person, name)}, \"Ada\", '2024-01-01'];"
            )
            report = database.terrain(as_of="2025-01-01")
            self.assertEqual(report["terrain_version"], positorium.TERRAIN_VERSION)
            self.assertEqual(report["resolved_as_of"], "'2025-01-01'")
            self.assertEqual(report["database"]["posits"], 1)

    def test_errors_are_specific_and_invalid_options_do_not_mutate(self):
        with positorium.Database.memory() as database:
            initial_roles = database.terrain()["database"]["roles"]
            with self.assertRaises(positorium.ParseError):
                database.execute("this is not Traqula;")
            with self.assertRaises(ValueError):
                database.execute("add role nope;", timeout=0)
            with self.assertRaises(TypeError):
                database.execute(
                    "add role nope;",
                    parameters={"value": "not a typed parameter"},
                )
            self.assertEqual(database.terrain()["database"]["roles"], initial_roles)


if __name__ == "__main__":
    unittest.main()
