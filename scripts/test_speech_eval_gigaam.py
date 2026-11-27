import hashlib
import tempfile
import unittest
from pathlib import Path

from gigaam_benchmark import verify_model_files


class GigaAMArtifactTest(unittest.TestCase):
    def test_rejects_modified_upstream_model_code(self):
        with tempfile.TemporaryDirectory() as folder:
            directory = Path(folder)
            model_code = directory / "modeling_gigaam.py"
            original = b"# verified fixture\n"
            model_code.write_bytes(original)
            artifacts = {model_code.name: hashlib.sha256(original).hexdigest()}
            verify_model_files(directory, artifacts)

            model_code.write_bytes(b"# modified fixture\n")
            with self.assertRaisesRegex(ValueError, "checksum mismatch for modeling_gigaam.py"):
                verify_model_files(directory, artifacts)


if __name__ == "__main__":
    unittest.main()
