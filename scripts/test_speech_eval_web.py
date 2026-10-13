"""Local annotation delivery and reference approval contracts, using synthetic audio."""
import hashlib
import http.client
import json
import tempfile
import threading
import unittest
from pathlib import Path

from speech_eval_web import ReviewStore, make_server


class WebReviewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.dataset = self.root / 'dataset'
        self.dataset.mkdir()
        audio = b'RIFF' + bytes(range(100))
        (self.dataset / 'test.wav').write_bytes(audio)
        row = {'id': 'clip-1', 'source': 'sotto-private-history', 'split': 'development',
               'audio': {'path': 'test.wav', 'sha256': hashlib.sha256(audio).hexdigest(),
                         'duration_seconds': 1.0}}
        (self.dataset / 'manifest.jsonl').write_text(json.dumps(row) + '\n', encoding='utf-8')
        self.catalog = self.root / 'catalog.json'
        self.catalog.write_text(json.dumps({'manifest_sha256': hashlib.sha256(
            (self.dataset / 'manifest.jsonl').read_bytes()).hexdigest(), 'batches': [{
                'id': '03', 'title': 'Batch 03', 'records': [{
                    'id': 'clip-1', 'ultra_text': 'Open example', 'run_sha256': 'a' * 64,
                    'parts': [{'question_id': '1.1'}, {'text': ' example'}],
                    'questions': [{'id': '1.1', 'prompt': 'Verb', 'source': 'Open',
                                   'options': [{'id': 'A', 'text': 'Open'},
                                               {'id': 'B', 'text': 'Close'}],
                                   'default': {'choice': 'B', 'custom': ''}}]}]}]}), encoding='utf-8')
        self.seed = self.root / 'seed.jsonl'
        self.seed.write_text('', encoding='utf-8')
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        (self.assets / 'index.html').write_text('<html>test</html>', encoding='utf-8')
        self.store = self.reopen()

    def reopen(self):
        return ReviewStore(self.dataset, self.catalog, self.seed, self.root / 'reviews')

    def tearDown(self):
        self.temp.cleanup()

    def payload(self, **changes):
        return dict({'id': 'clip-1', 'catalog_sha256': self.store.catalog_hash,
                     'revision': 0, 'mode': 'complete', 'asr_text': 'Close example',
                     'answers': {'1.1': {'choice': 'B', 'custom': ''}}}, **changes)

    def test_complete_review_is_durable_and_exportable(self):
        self.store.submit(self.payload())
        reopened = self.reopen()
        annotation = reopened.annotations()[0]
        self.assertEqual(annotation['asr_text'], 'Close example')
        self.assertEqual(annotation['reviewed_targets'], ['asr_text'])
        self.assertIsNone(annotation['clean_text'])
        self.assertEqual(reopened.state()['batches'][0]['records'][0]['review']['revision'], 1)
        self.assertEqual(json.loads((self.root / 'reviews' / 'annotations-web.jsonl').read_text()), annotation)

    def test_unknown_fragment_cannot_approve_full_draft(self):
        answers = {'1.1': {'choice': 'unclear', 'custom': ''}}
        with self.assertRaises(ValueError):
            self.store.submit(self.payload(answers=answers))
        self.store.submit(self.payload(answers=answers, mode='partial', asr_text=''))
        self.assertEqual(self.store.annotations(), [])
        with self.assertRaises(ValueError):
            self.store.submit(self.payload(revision=1, asr_text='Close [unclear]'))

    def test_partial_custom_review_does_not_approve_surrounding_words(self):
        self.store.submit(self.payload(mode='partial', answers={'1.1': {'choice': 'custom', 'custom': 'Keep'}}))
        annotation = self.store.annotations()[0]
        self.assertEqual(annotation['status'], 'fragments_reviewed')
        self.assertEqual(annotation['reviewed_targets'], [])
        self.assertIsNone(annotation['asr_text'])
        self.assertEqual(annotation['reviewed_fragments'][0]['asr_text'], 'Keep')

    def test_stale_tab_and_changed_catalog_cannot_overwrite_review(self):
        self.store.submit(self.payload())
        with self.assertRaises(ValueError):
            self.store.submit(self.payload())
        with self.assertRaises(ValueError):
            self.store.submit(self.payload(revision=1, catalog_sha256='old'))
        self.assertEqual(len(self.store.history), 1)

    def test_missing_question_and_empty_custom_are_rejected(self):
        for answers in ({}, {'1.1': {'choice': 'custom', 'custom': ' '}},
                        {'1.1': {'choice': ['B'], 'custom': ''}}):
            with self.assertRaises(ValueError):
                self.store.submit(self.payload(answers=answers))

    def test_seed_clean_target_is_retained_only_when_spoken_words_are_unchanged(self):
        row = {'id': 'clip-1', 'audio_sha256': hashlib.sha256((self.dataset / 'test.wav').read_bytes()).hexdigest(),
               'asr_text': 'Close example', 'clean_text': 'Close sample', 'status': 'reviewed',
               'reviewed_targets': ['asr_text', 'clean_text'], 'reviewer': 'user', 'method': 'audio_review'}
        self.seed.write_text(json.dumps(row) + '\n', encoding='utf-8')
        self.store = self.reopen()
        self.store.submit(self.payload())
        self.assertEqual(self.store.annotations()[0]['clean_text'], 'Close sample')
        self.store.submit(self.payload(revision=1, asr_text='Keep example'))
        self.assertIsNone(self.store.annotations()[0]['clean_text'])

    def test_changed_audio_is_rejected_before_serving(self):
        (self.dataset / 'test.wav').write_bytes(b'changed')
        with self.assertRaises(ValueError):
            self.reopen()

    def test_catalog_cannot_hide_the_remaining_transcript(self):
        catalog = json.loads(self.catalog.read_text(encoding='utf-8'))
        catalog['batches'][0]['records'][0]['parts'].pop()
        self.catalog.write_text(json.dumps(catalog), encoding='utf-8')
        with self.assertRaises(ValueError):
            self.reopen()

    def test_frozen_dataset_cannot_be_used_for_review_storage(self):
        with self.assertRaises(ValueError):
            ReviewStore(self.dataset, self.catalog, self.seed, self.dataset / 'reviews')

    def test_audio_ranges_and_local_request_boundaries(self):
        server = make_server(self.store, self.assets, port=0)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        host = f'127.0.0.1:{server.server_port}'
        connection = http.client.HTTPConnection(host)
        self.addCleanup(connection.close)
        connection.request('GET', '/audio/clip-1', headers={'Range': 'bytes=10-19'})
        response = connection.getresponse()
        self.assertEqual(response.status, 206)
        self.assertEqual(response.getheader('Content-Range'), 'bytes 10-19/104')
        self.assertEqual(len(response.read()), 10)
        connection.request('GET', '/audio/clip-1', headers={'Range': 'bytes=999-'})
        response = connection.getresponse()
        self.assertEqual(response.status, 416)
        response.read()
        connection.request('GET', '/audio/../../seed.jsonl')
        response = connection.getresponse()
        self.assertEqual(response.status, 404)
        response.read()
        connection.request('GET', '/api/state', headers={'Host': 'attacker.example'})
        response = connection.getresponse()
        self.assertEqual(response.status, 403)
        response.read()
        connection.request('POST', '/api/review', json.dumps(self.payload()), headers={
            'Content-Type': 'application/json', 'Origin': 'https://attacker.example',
            'X-Review-Token': self.store.token})
        response = connection.getresponse()
        self.assertEqual(response.status, 403)
        response.read()
        connection.request('POST', '/api/review', json.dumps(self.payload()), headers={
            'Content-Type': 'application/json', 'Origin': f'http://{host}',
            'X-Review-Token': self.store.token})
        response = connection.getresponse()
        self.assertEqual(response.status, 200)
        response.read()
        self.assertEqual(len(self.store.annotations()), 1)


if __name__ == '__main__':
    unittest.main()
