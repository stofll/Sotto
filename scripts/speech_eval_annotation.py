"""Prepare private annotation drafts from validated local runs; never approve them."""
import argparse
import collections
import hashlib
import json
from pathlib import Path
from speech_eval import normalize,read_jsonl,score,suspicious
from speech_eval_quality import approved_fragments, target_reviewed

def proposals(manifest,reports):
    if not any(r['metadata']['options'].get('--model')=='parakeet-ultra' for r in reports):
        raise ValueError('Parakeet Ultra primary run is required')
    by_id=collections.defaultdict(dict)
    for report in reports:
        model=report['metadata']['options']['--model']
        hints=report['metadata'].get('hints_mode','empty')
        for row in report['records']:
            by_id[row['id']][(model,hints)]={'model':model,'hints':hints,'text':row['text'],
                                          'run_sha256':report.get('run_sha256')}
    rows=[]
    for source in manifest:
        if source['source']!='sotto-private-history' or not source.get('audio') or target_reviewed(source['reference'],'asr_text') or approved_fragments(source['reference']):
            continue
        variants=list(by_id[source['id']].values())
        primary=next((v for v in variants if v['model']=='parakeet-ultra' and v['hints']=='empty'),None)
        if primary is None:
            continue
        models={v['model'] for v in variants}
        agreement={normalize(v['text']) for v in variants}
        flags={f"{v['model']}:{v['hints']}":suspicious(primary['text'],v['text']) for v in variants if v is not primary}
        rows.append({'id':source['id'],'source':source['source'],'language':source['language'],
                     'audio_sha256':source['audio']['sha256'],'audio_path':source['audio']['path'],
                     'duration_seconds':source['audio']['duration_seconds'],'asr_text':primary['text'],
                     'punctuated_text':None,'clean_text':None,'status':'proposed','reviewer':None,
                     'method':'local multi-model draft; Ultra primary; agreement is not audio verification',
                     'independent_models':len(models),'unanimous_normalized':len(models)>1 and len(agreement)==1,
                     'variants':variants,'disagreement_flags':flags})
    return sorted(rows,key=lambda r:(r['unanimous_normalized'],r['duration_seconds'],r['id']))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dataset',type=Path,required=True)
    parser.add_argument('--run',type=Path,action='append',required=True)
    parser.add_argument('--annotations',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.output.resolve().is_relative_to(args.dataset.resolve()):
        raise ValueError('Drafts must be outside the frozen dataset')
    reports=[]
    manifest={r['id']:r for r in read_jsonl(args.dataset/'manifest.jsonl')}
    for path in args.run:
        report=score(args.dataset,path,args.annotations)
        if report['metadata']['options']['--mode']!='stt':
            raise ValueError('Annotation drafts need raw STT runs')
        report['run_sha256']=hashlib.sha256(path.read_bytes()).hexdigest()
        for row in report['records']:
            manifest[row['id']]['reference']=row['reference']
        reports.append(report)
    rows=proposals(list(manifest.values()),reports)
    with args.output.open('x',encoding='utf-8',newline='\n') as stream:
        for row in rows: stream.write(json.dumps(row,ensure_ascii=False)+'\n')
    print(json.dumps({'proposed_records':len(rows),'unanimous_normalized':sum(r['unanimous_normalized'] for r in rows),
                      'reviewed_by_automation':0,'output':str(args.output)},ensure_ascii=True))


if __name__=='__main__':
    main()
