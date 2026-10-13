"""Serve private audio annotation on loopback; export reviewed references for the scorer."""
import argparse
import copy
import hashlib
import json
import mimetypes
import os
import re
import secrets
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit

from speech_eval import normalize, read_jsonl


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def atomic_write(path, text):
    temporary = path.with_name(path.name + '.tmp')
    with temporary.open('w', encoding='utf-8', newline='\n') as stream:
        stream.write(text)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


class ReviewStore:
    def __init__(self, dataset, catalog, seed, directory):
        self.dataset = dataset.resolve()
        self.directory = directory.resolve()
        if self.directory.is_relative_to(self.dataset):
            raise ValueError('Reviews must be outside the frozen dataset')
        self.catalog_hash = digest(catalog)
        self.seed_hash = digest(seed)
        self.catalog = json.loads(catalog.read_text(encoding='utf-8'))
        if self.catalog['manifest_sha256'] != digest(dataset / 'manifest.jsonl'):
            raise ValueError('Catalog manifest fingerprint mismatch')
        manifest = {row['id']: row for row in read_jsonl(dataset / 'manifest.jsonl')}
        self.seed = {row['id']: row for row in read_jsonl(seed)}
        if len(self.seed) != len(read_jsonl(seed)):
            raise ValueError('Duplicate seed annotation')
        for row in self.seed.values():
            source = manifest[row['id']]
            if row['audio_sha256'] != source['audio']['sha256']:
                raise ValueError('Seed audio fingerprint mismatch')
        self.records = {}
        for batch in self.catalog['batches']:
            for record in batch['records']:
                identity = record['id']
                if identity in self.records:
                    raise ValueError('Duplicate catalog record')
                source = manifest[identity]
                if source['source'] != 'sotto-private-history' or source['split'] != 'development':
                    raise ValueError('Only private development audio may enter this review queue')
                audio = (self.dataset / source['audio']['path']).resolve()
                if not audio.is_relative_to(self.dataset) or digest(audio) != source['audio']['sha256']:
                    raise ValueError('Catalog audio fingerprint mismatch')
                question_ids = [q['id'] for q in record['questions']]
                part_ids = [p['question_id'] for p in record['parts'] if 'question_id' in p]
                if len(set(question_ids)) != len(question_ids) or sorted(question_ids) != sorted(part_ids):
                    raise ValueError('Each question must identify exactly one transcript span')
                sources = {q['id']: q['source'] for q in record['questions']}
                original = ''.join(p['text'] if 'text' in p else sources[p['question_id']] for p in record['parts'])
                if original != record['ultra_text']:
                    raise ValueError('Transcript parts must cover the complete Ultra text')
                for question in record['questions']:
                    options = [o['id'] for o in question['options']]
                    if len(set(options)) != len(options) or any(o in ('custom', 'unclear', '') for o in options):
                        raise ValueError('Invalid option IDs')
                self.records[identity] = dict(record=record, source=source, audio=audio)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.ledger = self.directory / 'reviews.json'
        self.history = []
        if self.ledger.exists():
            saved = json.loads(self.ledger.read_text(encoding='utf-8'))
            if saved['catalog_sha256'] != self.catalog_hash or saved['seed_sha256'] != self.seed_hash:
                raise ValueError('Review store belongs to another catalog or seed; use a new directory')
            self.history = saved['history']
        self.lock = threading.Lock()
        self.token = secrets.token_urlsafe(32)
        self.export()

    def latest(self, identity):
        return next((row for row in reversed(self.history) if row['id'] == identity), None)

    def state(self):
        with self.lock:
            result = copy.deepcopy(self.catalog)
            result['catalog_sha256'] = self.catalog_hash
            result['token'] = self.token
            for batch in result['batches']:
                for record in batch['records']:
                    item = self.records[record['id']]
                    record['audio_url'] = '/audio/' + record['id']
                    record['audio_path'] = str(item['audio'])
                    record['duration_seconds'] = item['source']['audio']['duration_seconds']
                    record['review'] = self.latest(record['id'])
            return result

    def submit(self, payload):
        with self.lock:
            identity = payload.get('id')
            if not isinstance(identity, str) or identity not in self.records or payload.get('catalog_sha256') != self.catalog_hash:
                raise ValueError('Record or catalog changed; reload the page')
            previous = self.latest(identity)
            revision = previous['revision'] if previous else 0
            if type(payload.get('revision')) is not int or payload['revision'] != revision:
                raise ValueError('A newer review exists; reload before editing')
            mode = payload.get('mode')
            if mode not in ('complete', 'partial'):
                raise ValueError('Choose complete or partial review')
            questions = self.records[identity]['record']['questions']
            answers = payload.get('answers')
            if not isinstance(answers, dict) or set(answers) != {q['id'] for q in questions}:
                raise ValueError('Every question needs an answer')
            fragments = []
            for question in questions:
                answer = answers[question['id']]
                if not isinstance(answer, dict):
                    raise ValueError('Invalid answer')
                choice = answer.get('choice')
                if not isinstance(choice, str):
                    raise ValueError('Choice must be a string')
                options = {o['id']: o['text'] for o in question['options']}
                if choice in ('', 'unclear'):
                    if mode == 'complete':
                        raise ValueError('Unresolved speech cannot approve a whole transcript')
                    continue
                if choice == 'custom':
                    text = answer.get('custom')
                elif choice in options:
                    text = options[choice]
                else:
                    raise ValueError('Unknown choice')
                if not isinstance(text, str) or not normalize(text) or len(text) > 10000:
                    raise ValueError('Custom answer needs nonempty words')
                fragments.append({'id': question['id'], 'choice': choice,
                                  'source_fragment': question['source'], 'asr_text': text.strip()})
            text = payload.get('asr_text', '')
            if not isinstance(text, str) or len(text) > 30000 or (mode == 'complete' and (
                    not normalize(text) or re.search(r'\[(неразборчиво|unclear)\]', text, re.I))):
                raise ValueError('Full transcript needs nonempty words')
            review = {'id': identity, 'revision': revision + 1, 'mode': mode,
                      'answers': copy.deepcopy(answers), 'asr_text': text.strip() if mode == 'complete' else None,
                      'reviewed_fragments': fragments, 'reviewed_at': datetime.now(timezone.utc).isoformat()}
            history = self.history + [review]
            atomic_write(self.ledger, json.dumps({'catalog_sha256': self.catalog_hash,
                         'seed_sha256': self.seed_hash, 'history': history}, ensure_ascii=False, indent=2) + '\n')
            self.history = history
            self.export()
            return review

    def annotations(self):
        annotations = copy.deepcopy(self.seed)
        for identity, item in self.records.items():
            review = self.latest(identity)
            if review is None:
                continue
            annotation = {'id': identity, 'audio_sha256': item['source']['audio']['sha256'],
                          'audio_path': item['source']['audio']['path'],
                          'duration_seconds': item['source']['audio']['duration_seconds'],
                          'reviewer': 'user', 'method': 'local_web_audio_review',
                          'reviewed_at': review['reviewed_at'], 'revision': review['revision'],
                          'catalog_sha256': self.catalog_hash,
                          'status': 'asr_reviewed' if review['mode'] == 'complete' else 'fragments_reviewed',
                          'reviewed_targets': ['asr_text'] if review['mode'] == 'complete' else [],
                          'asr_text': review['asr_text'], 'clean_text': None, 'punctuated_text': None,
                          'notes': ['Word sequence only; punctuation, clean target and output usability are unreviewed.']}
            if review['mode'] == 'partial':
                annotation['reviewed_fragments'] = review['reviewed_fragments']
            else:
                # Free editing can supersede a selected span; the full reviewed text is authoritative.
                old = self.seed.get(identity, {})
                if normalize(old.get('asr_text') or '') == normalize(review['asr_text']):
                    for target in old.get('reviewed_targets', []):
                        if target != 'asr_text':
                            annotation[target] = old[target]
                            annotation['reviewed_targets'].append(target)
                if len(annotation['reviewed_targets']) > 1:
                    annotation['notes'] = ['Word sequence reviewed here; existing independently reviewed targets are retained because spoken words are unchanged.']
                terms = item['record'].get('terms', old.get('terms', []))
                words = ' ' + normalize(review['asr_text']) + ' '
                annotation['terms'] = [term for term in terms if ' ' + normalize(term['text']) + ' ' in words]
            if annotation['reviewed_targets'] or annotation.get('reviewed_fragments'):
                annotations[identity] = annotation
            else:
                annotations.pop(identity, None)
        return list(annotations.values())

    def export(self):
        atomic_write(self.directory / 'annotations-web.jsonl', ''.join(
            json.dumps(row, ensure_ascii=False) + '\n' for row in self.annotations()))


def make_server(store, assets, port=8765):
    assets = assets.resolve()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'

        def log_message(self, *args):
            pass

        def local(self):
            return self.headers.get('Host') in (f'127.0.0.1:{self.server.server_port}',
                                                f'localhost:{self.server.server_port}')

        def headers_for(self, status, content_type, size, extra=None):
            self.send_response(status)
            self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(size))
            self.send_header('Cache-Control', 'no-store')
            if self.close_connection:
                self.send_header('Connection', 'close')
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Referrer-Policy', 'no-referrer')
            self.send_header('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; media-src 'self'; connect-src 'self'; frame-ancestors 'none'")
            for key, value in (extra or {}).items():
                self.send_header(key, value)
            self.end_headers()

        def json_response(self, status, value):
            data = json.dumps(value, ensure_ascii=False).encode('utf-8')
            self.headers_for(status, 'application/json; charset=utf-8', len(data))
            if self.command != 'HEAD':
                self.wfile.write(data)

        def do_HEAD(self):
            self.do_GET()

        def do_GET(self):
            if not self.local():
                return self.json_response(403, {'error': 'Loopback host required'})
            path = unquote(urlsplit(self.path).path)
            if path == '/api/state':
                return self.json_response(200, store.state())
            if path == '/api/annotations':
                with store.lock:
                    return self.json_response(200, store.annotations())
            audio = path.startswith('/audio/')
            if audio:
                identity = path.removeprefix('/audio/')
                if identity not in store.records:
                    return self.json_response(404, {'error': 'Unknown recording'})
                file = store.records[identity]['audio']
            else:
                file = (assets / (path.lstrip('/') or 'index.html')).resolve()
                if not file.is_relative_to(assets) or not file.is_file():
                    return self.json_response(404, {'error': 'Unknown page'})
            size = file.stat().st_size
            start, end, status = 0, size - 1, 200
            extra = {'Accept-Ranges': 'bytes'} if audio else {}
            requested = self.headers.get('Range') if audio else None
            if requested:
                match = re.fullmatch(r'bytes=(\d*)-(\d*)', requested)
                if match and any(match.groups()):
                    left, right = match.groups()
                    if left:
                        start = int(left)
                        end = min(int(right), size - 1) if right else size - 1
                    else:
                        start = max(0, size - int(right))
                    status = 206
                if status != 206 or start > end or start >= size:
                    self.headers_for(416, 'audio/wav', 0, {'Content-Range': f'bytes */{size}'})
                    return
                extra['Content-Range'] = f'bytes {start}-{end}/{size}'
            content_type = 'audio/wav' if audio else mimetypes.guess_type(file.name)[0] or 'application/octet-stream'
            self.headers_for(status, content_type, end - start + 1, extra)
            if self.command != 'HEAD':
                with file.open('rb') as stream:
                    stream.seek(start)
                    remaining = end - start + 1
                    while remaining:
                        data = stream.read(min(65536, remaining))
                        if not data:
                            break
                        try:
                            self.wfile.write(data)
                        except (BrokenPipeError, ConnectionResetError):
                            break
                        remaining -= len(data)

        def do_POST(self):
            origin = f"http://{self.headers.get('Host')}"
            if not self.local() or self.headers.get('Origin') != origin or self.headers.get('X-Review-Token') != store.token:
                self.close_connection = True
                return self.json_response(403, {'error': 'Same-origin review token required'})
            if self.path != '/api/review' or self.headers.get('Content-Type') != 'application/json':
                self.close_connection = True
                return self.json_response(400, {'error': 'JSON review required'})
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= 100000:
                    raise ValueError('Invalid review size')
                payload = json.loads(self.rfile.read(size))
                if not isinstance(payload, dict):
                    raise ValueError('Review must be an object')
                review = store.submit(payload)
                self.json_response(200, {'review': review})
            except (ValueError, UnicodeDecodeError) as error:
                self.close_connection = True
                self.json_response(409, {'error': str(error)})
            except OSError:
                self.json_response(500, {'error': 'Could not write review; reload and retry'})

    return ThreadingHTTPServer(('127.0.0.1', port), Handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dataset', type=Path, required=True)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--seed', type=Path, required=True)
    parser.add_argument('--reviews', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--port', type=int, default=8765)
    args = parser.parse_args()
    if not (args.assets / 'index.html').is_file():
        raise ValueError('Build the annotation frontend before starting the server')
    store = ReviewStore(args.dataset, args.catalog, args.seed, args.reviews)
    server = make_server(store, args.assets, args.port)
    print(f'Local: http://127.0.0.1:{server.server_port}', flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == '__main__':
    main()
