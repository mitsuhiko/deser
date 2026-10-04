"""Captures the git config inputs of dulwich's test suite.

Runs tests/test_config.py of a dulwich source tree with ConfigFile.from_file
patched to append every (top level, not included) input to a JSON lines file
as {"test": ..., "input": <base64>}.

Usage: python capture-dulwich.py <dulwich-src> <out.jsonl>
"""

import base64
import io
import json
import os
import sys
import unittest

SRC, OUT = sys.argv[1], sys.argv[2]
sys.path.insert(0, SRC)

from dulwich.config import ConfigFile  # noqa: E402

current_test = [None]
records = []
original_from_file = ConfigFile.from_file.__func__


def from_file(cls, f, *args, **kwargs):
    if kwargs.get("include_depth", 0) == 0 and current_test[0] is not None:
        data = f.read()
        records.append(
            {"test": current_test[0], "input": base64.b64encode(data).decode()}
        )
        f = io.BytesIO(data)
    return original_from_file(cls, f, *args, **kwargs)


ConfigFile.from_file = classmethod(from_file)


class Recorder(unittest.TextTestResult):
    def startTest(self, test):
        current_test[0] = test.id().removeprefix("tests.test_config.")
        super().startTest(test)

    def stopTest(self, test):
        current_test[0] = None
        super().stopTest(test)


os.chdir(SRC)
suite = unittest.defaultTestLoader.loadTestsFromName("tests.test_config")
result = unittest.TextTestRunner(
    resultclass=Recorder, verbosity=0, stream=open(os.devnull, "w")
).run(suite)
if not result.wasSuccessful():
    sys.exit(f"dulwich's test_config failed: {result}")
with open(OUT, "w") as f:
    for record in records:
        f.write(json.dumps(record) + "\n")
