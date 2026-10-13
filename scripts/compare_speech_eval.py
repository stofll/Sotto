"""Compare frozen reports on exactly the same records, with paired coverage."""
import argparse
import collections
import hashlib
import json
import random
import statistics
from pathlib import Path
from speech_eval_quality import aggregate_quality


def _delta(before,after):
    return {'baseline':before,'candidate':after,'delta':after-before if before is not None and after is not None else None}


def _wer(pairs,samples):
    gold=[(a,b) for a,b in pairs if 'normalized_counts' in a]
    if not gold:
        return None
    words=sum(a['normalized_words'] for a,_ in gold)
    errors=lambda r:sum(r['normalized_counts'].values())
    before=sum(errors(a) for a,_ in gold)
    after=sum(errors(b) for _,b in gold)
    clusters=collections.defaultdict(lambda:[0,0])
    for a,b in gold:
        cluster=clusters[a['group_id']]
        cluster[0]+=errors(b)-errors(a)
        cluster[1]+=a['normalized_words']
    values=list(clusters.values())
    rng=random.Random(20261012)
    deltas=[]
    for _ in range(samples):
        sampled=[rng.choice(values) for _ in values]
        denominator=sum(v[1] for v in sampled)
        if denominator:
            deltas.append(sum(v[0] for v in sampled)/denominator)
    deltas.sort()
    return dict(_delta(before/words if words else None,after/words if words else None),
                baseline_errors=before,candidate_errors=after,reference_words=words,scored_records=len(gold),
                improved_records=sum(errors(b)<errors(a) for a,b in gold),
                regressed_records=sum(errors(b)>errors(a) for a,b in gold),
                unchanged_records=sum(errors(b)==errors(a) for a,b in gold),clusters=len(values),
                delta_ci95=[deltas[int((len(deltas)-1)*p)] for p in (.025,.975)] if len(values)>1 and deltas else None)


def _quality(pairs):
    fields={'clean_word_edits':('clean_word_edits','per_100_words'),
            'ready_without_edits':('ready_without_edits','rate'),
            'editing_seconds':('editing_time','seconds_per_audio_minute'),
            'terms':('terms','accuracy'),'reviewed_fragments':('reviewed_fragments','accuracy'),
            'correct_input_damage':('correct_input_damage','word_edits'),
            **{k:(k,'count') for k in ('mandatory_word_edits','punctuation_edits','false_term_replacements','meaning_errors','postprocess_damage')}}
    result={}
    for key,(aggregate,value) in fields.items():
        eligible=[(a['quality'],b['quality']) for a,b in pairs if key in a['quality'] and key in b['quality']]
        left=aggregate_quality([a for a,_ in eligible])[aggregate]
        right=aggregate_quality([b for _,b in eligible])[aggregate]
        result[aggregate]=dict(_delta(left[value],right[value]),paired_records=len(eligible),
                               baseline_details=left,candidate_details=right)
    error_keys=lambda quality:{(e['id'],) if e.get('id') else (e['kind'],e['note']) for e in quality['meaning_errors']}
    meaning_pairs=[(a,b) for a,b in pairs if 'meaning_errors' in a['quality'] and 'meaning_errors' in b['quality']]
    result['new_confirmed_meaning_error_records']=[a['id'] for a,b in pairs
        if 'meaning_errors' in a['quality'] and 'meaning_errors' in b['quality']
        and error_keys(b['quality'])-error_keys(a['quality'])] if meaning_pairs else None
    paragraphs=[(a['quality']['paragraph_judgement'],b['quality']['paragraph_judgement']) for a,b in pairs
                if 'paragraph_judgement' in a['quality'] and 'paragraph_judgement' in b['quality']]
    result['paragraphs']={'paired_records':len(paragraphs),
                          'transitions':{f'{a}->{b}':n for (a,b),n in collections.Counter(paragraphs).items()}}
    return result


def _cer(pairs):
    gold=[(a,b) for a,b in pairs if 'normalized_char_errors' in a and 'normalized_char_errors' in b]
    if not gold:
        return None
    chars=sum(a['normalized_chars'] for a,_ in gold)
    before=sum(a['normalized_char_errors'] for a,_ in gold)
    after=sum(b['normalized_char_errors'] for _,b in gold)
    return dict(_delta(before/chars if chars else None,after/chars if chars else None),
                baseline_errors=before,candidate_errors=after,reference_chars=chars,scored_records=len(gold))


def compare(baseline,candidate,bootstrap_samples=1000):
    if bootstrap_samples<1:
        raise ValueError('Bootstrap samples must be positive')
    for key in ('manifest_sha256','annotations_sha256','normalization'):
        if baseline.get(key)!=candidate.get(key):
            raise ValueError(f'Comparison {key} mismatch; re-score both runs with identical references')
    for key in ('compute','threads','cpu','os'):
        if baseline['metadata'].get(key)!=candidate['metadata'].get(key):
            raise ValueError(f'Timing context {key} mismatch')
    for key in ('--mode','--split'):
        if baseline['metadata']['options'][key]!=candidate['metadata']['options'][key]:
            raise ValueError('Mode/split mismatch')
    left={r['id']:r for r in baseline['records']}
    right={r['id']:r for r in candidate['records']}
    if not left or left.keys()!=right.keys() or len(left)!=len(baseline['records']) or len(right)!=len(candidate['records']):
        raise ValueError('Paired comparison needs exactly the same unique IDs')
    groups=collections.defaultdict(list)
    for key,a in left.items():
        b=right[key]
        if baseline['metadata']['options']['--mode']=='format' and (not isinstance(a.get('input'),str) or a['input']!=b.get('input')):
            raise ValueError(f'Paired record {key}: input mismatch; formatter comparisons need identical input text')
        for field in ('source','language','split','group_id','audio_sha256','audio_seconds','reference','normalized_words','normalized_chars'):
            if a.get(field)!=b.get(field):
                raise ValueError(f'Paired record {key}: {field} mismatch')
        if ('normalized_counts' in a)!=('normalized_counts' in b):
            raise ValueError('Scored coverage mismatch')
        secs=a['audio_seconds']
        duration='text-only' if secs is None else '<15s' if secs<15 else '15-60s' if secs<60 else '60-120s' if secs<120 else '>=120s'
        for bucket in ('all',duration):
            groups[(a['source'],a['language'],bucket)].append((a,b))
    summary=[]
    for (source,language,duration),pairs in sorted(groups.items(),key=lambda item:str(item[0])):
        warm=[(a,b) for a,b in pairs if not a['first_after_load'] and not b['first_after_load']]
        latency={}
        for name,fn in [('median_ms',statistics.median),('p95_ms',lambda v:sorted(v)[min(len(v)-1,int(len(v)*.95))])]:
            before=fn([a['elapsed_ms'] for a,_ in warm]) if warm else None
            after=fn([b['elapsed_ms'] for _,b in warm]) if warm else None
            latency[name]=dict(_delta(before,after),relative_delta=after/before-1 if before and after is not None else None,paired_records=len(warm))
        summary.append(dict(source=source,language=language,duration=duration,records=len(pairs),
                            wer=_wer(pairs,bootstrap_samples),cer=_cer(pairs),quality=_quality(pairs),latency=latency))
    return dict(schema=1,baseline_metadata=baseline['metadata'],candidate_metadata=candidate['metadata'],
                method='Identical IDs and references; micro WER; paired metric-specific coverage; deterministic sentence-group bootstrap for WER delta. CI does not correct for model pretraining overlap or multiple comparisons.',
                bootstrap_samples=bootstrap_samples,groups=summary)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline',type=Path,required=True)
    parser.add_argument('--candidate',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    result=compare(json.loads(args.baseline.read_text(encoding='utf-8')),json.loads(args.candidate.read_text(encoding='utf-8')))
    result['report_sha256']={name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in [('baseline',args.baseline),('candidate',args.candidate)]}
    with args.output.open('x',encoding='utf-8',newline='\n') as stream:
        json.dump(result,stream,ensure_ascii=False,indent=2)
        stream.write('\n')
    print(json.dumps({'paired_records':len(json.loads(args.baseline.read_text(encoding='utf-8'))['records']),'output':str(args.output)},ensure_ascii=True))


if __name__=='__main__':
    main()
