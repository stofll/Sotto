"""Reviewed quality metrics; unreviewed outputs must never become ground truth."""
import collections
import math


def target_reviewed(reference, target):
    targets = reference.get('reviewed_targets')
    if targets is not None:
        return target in targets
    statuses = {'asr_text': ('reviewed','asr_reviewed','publisher_provided'),
                'clean_text': ('reviewed','clean_reviewed'),
                'punctuated_text': ('reviewed','punctuation_reviewed')}
    return reference.get('status') in statuses.get(target, ())


def approved_fragments(reference):
    if reference.get('status') in ('fragments_reviewed','asr_reviewed','clean_reviewed',
                                  'reviewed','target_reviewed','publisher_provided'):
        return reference.get('reviewed_fragments')
    return None


def _number(value, name, integer=False):
    if type(value) not in (int,float) or not math.isfinite(value) or value < 0 or (integer and type(value) is not int):
        raise ValueError(f'{name} must be a finite nonnegative number')
    return value


def _phrase_count(words, phrases):
    if not phrases or any(not phrase for phrase in phrases):
        raise ValueError('Term cannot be empty')
    phrases = sorted(phrases,key=len,reverse=True)
    count, index = 0, 0
    while index < len(words):
        phrase = next((p for p in phrases if words[index:index+len(p)] == p),None)
        if phrase:
            count += 1
            index += len(phrase)
        else:
            index += 1
    return count


def record_quality(record, reference, normalize, alignment, review=None, mode='format'):
    output = normalize(record['text']).split()
    result = {'audio_seconds':record.get('audio_seconds')}
    fragments=approved_fragments(reference)
    if fragments is not None:
        if not isinstance(fragments,list) or not fragments:
            raise ValueError('Reviewed fragments need a nonempty list')
        seen,details=set(),[]
        remaining_mentions={}
        for fragment in fragments:
            key=fragment.get('id')
            target='clean_text' if mode=='format' and fragment.get('clean_text') is not None else 'asr_text'
            text=fragment.get(target)
            exclusions=fragment.get('clean_exclusions',[]) if target=='clean_text' else []
            if not isinstance(key,str) or not key or key in seen or not isinstance(text,str) or not normalize(text):
                raise ValueError('Reviewed fragments need unique IDs and nonempty target text')
            if not isinstance(exclusions,list) or any(not isinstance(v,str) or not normalize(v) for v in exclusions):
                raise ValueError('Fragment exclusions need nonempty phrases')
            seen.add(key)
            phrase=tuple(normalize(text).split())
            if phrase not in remaining_mentions:
                remaining_mentions[phrase]=_phrase_count(output,[list(phrase)])
            matched=remaining_mentions[phrase]>0
            matched=matched and not any(_phrase_count(output,[normalize(v).split()]) for v in exclusions)
            if matched:
                remaining_mentions[phrase]-=1
            details.append({'id':key,'target':target,'matched':matched})
        result['reviewed_fragments']={'count':len(details),'matched':sum(d['matched'] for d in details),'details':details}
    clean = reference.get('clean_text')
    if clean is not None and target_reviewed(reference,'clean_text'):
        words = normalize(clean).split()
        result['clean_word_edits'] = {'count':sum(op!='equal' for op,_,_ in alignment(words,output)), 'words':len(words)}
    asr = reference.get('asr_text')
    if asr is not None and target_reviewed(reference,'asr_text'):
        words = normalize(asr).split()
        if normalize(record.get('input','')).split() == words:
            targets=[words]
            if clean is not None and target_reviewed(reference,'clean_text'):
                targets.append(normalize(clean).split())
            result['correct_input_damage'] = min(sum(op!='equal' for op,_,_ in alignment(target,output)) for target in targets)
        terms = reference.get('terms')
        if terms is not None:
            seen, total, correct, extra = set(), 0, 0, 0
            for term in terms:
                spellings = term.get('accepted',[term['text']])
                if not isinstance(spellings,list) or not spellings or any(not isinstance(v,str) or not normalize(v) for v in spellings):
                    raise ValueError('Term variants need a list of nonempty spellings')
                key = normalize(term['text'])
                if key in seen:
                    raise ValueError('Duplicate reviewed term')
                seen.add(key)
                mentions = _phrase_count(words,[key.split()])
                if not mentions:
                    raise ValueError('Reviewed term absent from verbatim reference')
                variants = {normalize(v) for v in spellings}
                found = _phrase_count(output,[v.split() for v in variants])
                total += mentions
                correct += min(mentions,found)
                extra += max(0,found-mentions)
            result['terms'] = {'mentions':total,'correct':correct,'extra':extra}
    if review is None:
        return result
    if 'ready_without_edits' in review:
        if type(review['ready_without_edits']) is not bool:
            raise ValueError('ready_without_edits must be boolean')
        result['ready_without_edits'] = review['ready_without_edits']
    for field in ('editing_seconds','mandatory_word_edits','punctuation_edits','false_term_replacements'):
        if field in review:
            result[field] = _number(review[field],field,integer=field!='editing_seconds')
    for field in ('meaning_errors','postprocess_damage'):
        if field in review:
            if not isinstance(review[field],list) or any(not isinstance(e,dict) or not e.get('kind') or not e.get('note') for e in review[field]):
                raise ValueError(f'{field} needs a list of kind/note entries')
            result[field] = review[field]
    if 'paragraph_judgement' in review:
        if review['paragraph_judgement'] not in ('keep','split','merge','mixed'):
            raise ValueError('Unknown paragraph judgement')
        result['paragraph_judgement'] = review['paragraph_judgement']
    return result


def aggregate_quality(records):
    fragments=[r['reviewed_fragments'] for r in records if 'reviewed_fragments' in r]
    fragment_count=sum(r['count'] for r in fragments)
    fragment_matched=sum(r['matched'] for r in fragments)
    clean = [r['clean_word_edits'] for r in records if 'clean_word_edits' in r]
    words = sum(r['words'] for r in clean)
    edits = sum(r['count'] for r in clean)
    ready = [r['ready_without_edits'] for r in records if 'ready_without_edits' in r]
    timed = [r for r in records if 'editing_seconds' in r and (r.get('audio_seconds') or 0)>0]
    seconds = sum(r['editing_seconds'] for r in timed)
    audio = sum(r['audio_seconds'] for r in timed)
    terms = [r['terms'] for r in records if 'terms' in r]
    mentions = sum(r['mentions'] for r in terms)
    correct = sum(r['correct'] for r in terms)
    damage = [r['correct_input_damage'] for r in records if 'correct_input_damage' in r]
    result = {
        'reviewed_fragments':{'reviewed_records':len(fragments),'fragments':fragment_count,'matched':fragment_matched,
                              'accuracy':fragment_matched/fragment_count if fragment_count else None,
                              'method':'Normalized whole phrase presence with reviewed exclusions; scoped fragments, not whole-utterance WER or usability'},
        'clean_word_edits':{'reviewed_records':len(clean),'count':edits if clean else None,'reference_words':words,'per_100_words':100*edits/words if words else None,'method':'minimal word edit proxy against reviewed clean text; not measured keyboard actions'},
        'ready_without_edits':{'reviewed_records':len(ready),'accepted':sum(ready),'rate':sum(ready)/len(ready) if ready else None},
        'editing_time':{'reviewed_records':len(timed),'editing_seconds':seconds if timed else None,'audio_seconds':audio,'seconds_per_audio_minute':60*seconds/audio if audio else None},
        'terms':{'reviewed_records':len(terms),'mentions':mentions,'correct':correct,'accuracy':correct/mentions if mentions else None,'extra_mentions':sum(r['extra'] for r in terms) if terms else None,'method':'whole reviewed mention matching; position/meaning need separate review'},
        'correct_input_damage':{'eligible_records':len(damage),'affected_records':sum(d>0 for d in damage),'word_edits':sum(damage) if damage else None,'method':'minimum lexical deviation from verbatim or reviewed clean target on verbatim-correct fixed input; not semantic adjudication'},
    }
    for field in ('meaning_errors','postprocess_damage'):
        reviewed = [r[field] for r in records if field in r]
        kinds = collections.Counter(e['kind'] for group in reviewed for e in group)
        result[field] = {'reviewed_records':len(reviewed),'affected_records':sum(bool(r) for r in reviewed),'count':sum(map(len,reviewed)) if reviewed else None,'by_kind':dict(kinds)}
    for field in ('mandatory_word_edits','punctuation_edits','false_term_replacements'):
        reviewed = [r[field] for r in records if field in r]
        result[field] = {'reviewed_records':len(reviewed),'count':sum(reviewed) if reviewed else None}
    result['paragraphs'] = dict(collections.Counter(r['paragraph_judgement'] for r in records if 'paragraph_judgement' in r))
    return result
