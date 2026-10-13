"""Score local model_benchmark --eval JSONL output without third-party services."""
import argparse
import collections
import difflib
import hashlib
import itertools
import json
import re
import statistics
import unicodedata
from pathlib import Path
from speech_eval_quality import aggregate_quality, record_quality, target_reviewed


NEGATION_WORDS = frozenset(('не','ни','нет','без','not','no','never','without',"don't", "isn't", "can't"))
# Finite RU/EN lexical markers; changed spans require review, not a semantic verdict.
NUMBER_WORDS = frozenset('''
    ноль нуль нуля нулю нулём нулем нуле
    один одна одно одни одного одной одному одним одном
    два две двух двум двумя три трёх трех трём трем тремя
    четыре четырёх четырех четырём четырем четырьмя
    пять пяти пятью шесть шести шестью семь семи семью
    восемь восьми восемью девять девяти девятью
    десять одиннадцать двенадцать тринадцать четырнадцать пятнадцать
    шестнадцать семнадцать восемнадцать девятнадцать
    двадцать тридцать сорок пятьдесят шестьдесят семьдесят восемьдесят девяносто
    сто двести триста четыреста пятьсот шестьсот семьсот восемьсот девятьсот
    тысяча тысячи тысяч миллион миллиона миллионов миллиард миллиарда миллиардов
    zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen
    fifteen sixteen seventeen eighteen nineteen twenty thirty forty fifty sixty seventy eighty ninety
    hundred hundreds thousand thousands million millions billion billions
'''.split())


def normalize(text):
    text = unicodedata.normalize('NFC', text).casefold().replace('’', "'")
    text = ''.join(c if c.isalnum() or c.isspace() or c == "'" else ' ' for c in text)
    return ' '.join(text.split())


def alignment(reference, hypothesis):
    """Levenshtein, with deterministic equal/substitute/delete/insert tie order."""
    costs = [list(range(len(hypothesis) + 1))]
    for i, ref in enumerate(reference, 1):
        row = [i]
        for j, hyp in enumerate(hypothesis, 1):
            row.append(min(costs[i-1][j-1] + (ref != hyp), costs[i-1][j] + 1, row[j-1] + 1))
        costs.append(row)
    i, j, out = len(reference), len(hypothesis), []
    while i or j:
        if i and j and costs[i][j] == costs[i-1][j-1] + (reference[i-1] != hypothesis[j-1]):
            out.append(('equal' if reference[i-1] == hypothesis[j-1] else 'substitute', reference[i-1], hypothesis[j-1]))
            i, j = i-1, j-1
        elif i and costs[i][j] == costs[i-1][j] + 1:
            out.append(('delete', reference[i-1], None))
            i -= 1
        else:
            out.append(('insert', None, hypothesis[j-1]))
            j -= 1
    return out[::-1]


def distance(reference, hypothesis):
    previous = list(range(len(hypothesis) + 1))
    for i, ref in enumerate(reference, 1):
        row = [i]
        for j, hyp in enumerate(hypothesis, 1):
            row.append(min(previous[j-1] + (ref != hyp), previous[j] + 1, row[-1] + 1))
        previous = row
    return previous[-1]


def counts(ops):
    return dict(collections.Counter(op for op, _, _ in ops if op != 'equal'))


def _critical_changes(left, right):
    changes = {'negation_changes':[],'numeric_changes':[]}
    if left == right:
        return changes
    left_index, right_index = 0, 0
    for unchanged, grouped in itertools.groupby(alignment(left,right),key=lambda op:op[0]=='equal'):
        diff = list(grouped)
        before = [a for _,a,_ in diff if a is not None]
        after = [b for _,_,b in diff if b is not None]
        if not unchanged:
            tokens = before + after
            negation = any(token in NEGATION_WORDS for token in tokens)
            numeric = any(token in NUMBER_WORDS or any(c.isdigit() for c in token) for token in tokens)
            if negation or numeric:
                span = {'before':' '.join(before),'after':' '.join(after),
                        'before_context':' '.join(left[max(0,left_index-3):left_index+len(before)+3]),
                        'after_context':' '.join(right[max(0,right_index-3):right_index+len(after)+3]),
                        'diff':diff}
                if negation:
                    changes['negation_changes'].append(span)
                if numeric:
                    changes['numeric_changes'].append(span)
        left_index += len(before)
        right_index += len(after)
    return changes


def suspicious(before, after):
    left, right = normalize(before).split(), normalize(after).split()
    removed = collections.Counter(left) - collections.Counter(right)
    return {
        **_critical_changes(left,right),
        'negations_removed': {k:v for k,v in removed.items() if k in NEGATION_WORDS},
        'numeric_tokens_removed': {k:v for k,v in removed.items() if any(c.isdigit() for c in k)},
        'latin_tokens_removed': {k:v for k,v in removed.items() if re.search('[a-z]', k)},
        'word_loss_fraction': max(0, (len(left)-len(right))/len(left)) if left else 0,
        'empty_output': bool(before.strip()) and not after.strip(),
    }


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines() if line.strip()]


def _reviews(path, results, records):
    if path is None:
        return {}
    run_hash = hashlib.sha256(results.read_bytes()).hexdigest()
    by_id = {r['id']:r for r in records}
    reviewed = {}
    for review in read_jsonl(path):
        if review.get('status') != 'reviewed':
            continue
        record = by_id.get(review['id'])
        if not record or review.get('run_sha256') != run_hash or review.get('output_sha256') != hashlib.sha256(record['text'].encode('utf-8')).hexdigest():
            raise ValueError('Output review fingerprint mismatch')
        if not review.get('reviewer') or not review.get('method') or review['id'] in reviewed:
            raise ValueError('Output review needs unique ID, reviewer and method')
        reviewed[review['id']] = review
    return reviewed


def score(dataset, results, annotations=None, reviews=None):
    manifest_bytes = (dataset / 'manifest.jsonl').read_bytes()
    manifest = {r['id']:r for r in read_jsonl(dataset / 'manifest.jsonl')}
    annotations_hash = None
    if annotations is not None:
        annotations_hash = hashlib.sha256(annotations.read_bytes()).hexdigest()
        seen = set()
        for annotation in read_jsonl(annotations):
            if annotation['id'] in seen:
                raise ValueError('Duplicate annotation')
            seen.add(annotation['id'])
            if annotation.get('status') not in ('asr_reviewed','clean_reviewed','punctuation_reviewed','fragments_reviewed','reviewed'):
                continue
            source = manifest[annotation['id']]
            if source['source'] != 'sotto-private-history' or not source['audio'] or annotation.get('audio_sha256') != source['audio']['sha256']:
                raise ValueError('Annotation audio/source mismatch')
            inferred = {'asr_reviewed':['asr_text'],'clean_reviewed':['clean_text'],
                        'punctuation_reviewed':['punctuated_text'],
                        'fragments_reviewed':[],
                        'reviewed':[k for k in ('asr_text','punctuated_text','clean_text') if annotation.get(k) is not None]}
            targets = annotation.get('reviewed_targets',inferred[annotation['status']])
            if not annotation.get('reviewer') or not annotation.get('method') or (not targets and not annotation.get('reviewed_fragments')) or any(k not in ('asr_text','punctuated_text','clean_text') or not isinstance(annotation.get(k),str) for k in targets):
                raise ValueError('Reviewed annotation needs reviewer, method and target text')
            reference = dict(source['reference'],reviewer=annotation['reviewer'],method=annotation['method'],notes=annotation.get('notes'))
            for target in targets:
                reference[target] = annotation[target]
            reference['reviewed_targets'] = targets
            reference['status'] = 'reviewed' if 'asr_text' in targets else 'target_reviewed'
            if 'terms' in annotation and 'asr_text' in targets:
                reference['terms'] = annotation['terms']
            if 'reviewed_fragments' in annotation:
                reference['reviewed_fragments']=annotation['reviewed_fragments']
            source['reference'] = reference
    rows = read_jsonl(results)
    if not rows or rows[0].get('type') != 'run' or rows[-1].get('type') != 'complete':
        raise ValueError('Incomplete run; preserve checkpoint and run into a new output file')
    metadata = rows[0]
    records = [r for r in rows if r['type'] == 'record']
    if len(records) != metadata['records'] or len(records) != rows[-1]['records'] or len({r['id'] for r in records}) != len(records):
        raise ValueError('Missing or duplicated results')
    mode = metadata['options']['--mode']
    if mode == 'regressions':
        failures = [r['id'] for r in records if r['text'] != r['expected'] or r['preview'] != r['preview_expected']]
        return {'metadata':metadata, 'cases':len(records), 'failures':failures}
    output_reviews = _reviews(reviews,results,records)
    groups, details = collections.defaultdict(list), []
    for record in records:
        source = manifest[record['id']]
        if source['split'] != metadata['options']['--split'] or record['split'] != source['split']:
            raise ValueError('Split mismatch')
        if record['language'] != source['language'] or record['source'] != source['source']:
            raise ValueError('Record context mismatch')
        if mode == 'stt':
            expected_hash = source['audio']['sha256'] if source['audio'] else None
            actual_hash = record.get('audio_sha256')
            # Legacy full-manifest runs predate the per-record audio fingerprint.
            legacy = actual_hash is None and metadata.get('input_sha256') == hashlib.sha256(manifest_bytes).hexdigest() and '--input' not in metadata['options']
            if not legacy and (not expected_hash or actual_hash != expected_hash):
                raise ValueError('ASR audio fingerprint differs from the frozen reference')
        ref = source['reference']
        before = record['input'] if mode == 'format' else (ref['asr_text'] or source['observed']['raw_text'] or '')
        item = dict(record, group_id=source.get('group_id',source['id']), reference=ref, flags=suspicious(before, record['text']),
                    input_diff=list(difflib.ndiff(before.split(), record['text'].split())),
                    quality=record_quality(record,ref,normalize,alignment,output_reviews.get(record['id']),mode=mode))
        if mode != 'format':
            item['quality'].pop('correct_input_damage',None)
        if ref['asr_text'] is not None and target_reviewed(ref,'asr_text'):
            norm_ref, norm_hyp = normalize(ref['asr_text']), normalize(record['text'])
            ops = alignment(norm_ref.split(), norm_hyp.split())
            raw_ops = alignment(ref['asr_text'].split(), record['text'].split())
            raw_ref = ref['punctuated_text'] or ref['asr_text']
            item.update(normalized_counts=counts(ops), normalized_diff=[o for o in ops if o[0] != 'equal'],
                        normalized_words=len(norm_ref.split()), normalized_chars=len(norm_ref),
                        normalized_char_errors=distance(norm_ref, norm_hyp),
                        raw_counts=counts(raw_ops), raw_words=len(ref['asr_text'].split()),
                        raw_chars=len(raw_ref), raw_char_errors=distance(raw_ref, record['text']),
                        publisher_diff=list(difflib.ndiff(raw_ref.split(),record['text'].split())))
        details.append(item)
        secs = source['audio']['duration_seconds'] if source['audio'] else None
        bucket = 'text-only' if secs is None else '<15s' if secs < 15 else '15-60s' if secs < 60 else '60-120s' if secs < 120 else '>=120s'
        for duration in ('all',bucket):
            groups[(source['source'], source['language'],duration)].append(item)
    summary = []
    for (source, lang, duration), items in sorted(groups.items(),key=lambda x:str(x[0])):
        times = [r['elapsed_ms'] for r in items if not r['first_after_load']]
        totals = {'source':source,'language':lang,'duration':duration,'records':len(items),
                  'changed_records':sum(r['input'] != r['text'] for r in items) if mode == 'format' else None,
                  'median_ms_warm':statistics.median(times) if times else None,
                  'p95_ms_warm':sorted(times)[min(len(times)-1,int(len(times)*.95))] if times else None,
                  'audio_seconds':sum(r['audio_seconds'] or 0 for r in items),
                  'suspicious_records':sum(any(r['flags'][k] for k in ('negation_changes','numeric_changes','negations_removed','numeric_tokens_removed','empty_output')) or r['flags']['word_loss_fraction'] > .2 for r in items)}
        totals['quality'] = aggregate_quality([r['quality'] for r in items])
        seconds = sum(r['audio_seconds'] or 0 for r in items if not r['first_after_load'])
        totals['warm_rtf'] = sum(times)/1000/seconds if seconds and mode == 'stt' else None
        gold = [r for r in items if 'normalized_counts' in r]
        totals['scored_records'] = len(gold)
        if gold:
            for style in ('normalized','raw'):
                errors = collections.Counter()
                for r in gold: errors.update(r[f'{style}_counts'])
                nwords = sum(r[f'{style}_words'] for r in gold)
                nchars = sum(r[f'{style}_chars'] for r in gold)
                char_errors = sum(r[f'{style}_char_errors'] for r in gold)
                totals[style] = {'counts':dict(errors),'words':nwords,'wer':sum(errors.values())/nwords if nwords else None,
                                 'char_errors':char_errors,'chars':nchars,'cer':char_errors/nchars if nchars else None}
            substitutions = collections.Counter((a,b) for r in gold for op,a,b in r['normalized_diff'] if op == 'substitute')
            totals['frequent_substitutions'] = [{'reference':a,'hypothesis':b,'count':n} for (a,b),n in substitutions.most_common(20)]
        summary.append(totals)
    first = records[0]
    return {'schema':2,'metadata':metadata,'manifest_sha256':hashlib.sha256(manifest_bytes).hexdigest(), 'annotations_sha256':annotations_hash,
            'timing':{'load_ms':next((r['load_ms'] for r in rows if r['type']=='load'),None),
                      'first_inference_ms':first['elapsed_ms'],
                      'first_clip_repeat_ms':(first.get('repeat') or {}).get('elapsed_ms'),
                      'scope':'prepared audio inference only; no capture, VAD or paste' if mode=='stt' else 'formatter only'},
            'reviews_sha256':hashlib.sha256(reviews.read_bytes()).hexdigest() if reviews else None,
            'normalization':"NFC, Unicode casefold, curly apostrophe to ASCII; other punctuation -> spaces; whitespace collapsed. ё preserved; numbers not rewritten; apostrophes and negations retained. WER micro-average; CER includes spaces. Raw WER uses asr_text, raw CER uses publisher punctuation proxy; raw diffs saved.",
            'summary':summary,'records':details}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dataset',type=Path,required=True)
    parser.add_argument('--results',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--annotations',type=Path)
    parser.add_argument('--reviews',type=Path)
    args = parser.parse_args()
    dataset = args.dataset.resolve()
    if args.output.resolve().is_relative_to(dataset):
        raise ValueError('Results must be outside the frozen dataset')
    report = score(dataset,args.results,args.annotations,args.reviews)
    with args.output.open('x',encoding='utf-8',newline='\n') as stream:
        json.dump(report,stream,ensure_ascii=False,indent=2)
        stream.write('\n')
    print(json.dumps({k:v for k,v in report.items() if k in ('summary','cases','failures')},ensure_ascii=True,indent=2))


if __name__ == '__main__':
    main()
