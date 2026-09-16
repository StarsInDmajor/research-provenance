import copy
import tempfile
import unittest
from pathlib import Path

import run_acceptance as runner


class CorpusVerificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / '.research').mkdir()
        (self.root / '.research/project.yaml').write_bytes(b'{}\n')
        entries = [{'path': '.research/project.yaml', 'sha256': runner.sha256_bytes(b'{}\n')}]
        self.manifest = {
            'sorted_path_sha256_entries': entries,
            'aggregate_sha256': runner.sha256_bytes(
                b'[{"path":".research/project.yaml","sha256":"' + entries[0]['sha256'].encode() + b'"}]'
            ),
        }

    def test_exact_corpus_passes(self):
        runner.verify_corpus(self.root, self.manifest)

    def test_mutated_missing_extra_or_symlink_file_fails(self):
        target = self.root / '.research/project.yaml'
        target.write_bytes(b'[]\n')
        with self.assertRaises(RuntimeError):
            runner.verify_corpus(self.root, self.manifest)
        target.unlink()
        with self.assertRaises(RuntimeError):
            runner.verify_corpus(self.root, self.manifest)
        target.write_bytes(b'{}\n')
        extra = self.root / 'extra'
        extra.write_bytes(b'{}\n')
        with self.assertRaises(RuntimeError):
            runner.verify_corpus(self.root, self.manifest)
        target.unlink()
        target.symlink_to(extra)
        with self.assertRaises(RuntimeError):
            runner.verify_corpus(self.root, self.manifest)

    def test_bad_aggregate_duplicate_and_escape_fail(self):
        for change in ('aggregate', 'duplicate', 'escape'):
            manifest = copy.deepcopy(self.manifest)
            if change == 'aggregate':
                manifest['aggregate_sha256'] = 'sha256:' + '0' * 64
            elif change == 'duplicate':
                manifest['sorted_path_sha256_entries'] *= 2
            else:
                manifest['sorted_path_sha256_entries'][0]['path'] = '../outside'
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                runner.verify_corpus(self.root, manifest)


if __name__ == '__main__':
    unittest.main()
