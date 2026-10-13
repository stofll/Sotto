import unittest
import json
import hashlib
import tempfile
from pathlib import Path
from speech_eval import alignment, distance, normalize, score, suspicious


class ScoringTests(unittest.TestCase):
    def test_fragment_review_never_approves_surrounding_asr_draft(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            row=dict(id='private',source='sotto-private-history',split='development',language='ru',
                     audio={'sha256':'abc','duration_seconds':1},observed={'raw_text':'черновик центр выплаты'},
                     reference={'status':'unreviewed','asr_text':None,'punctuated_text':None,'clean_text':None})
            (root/'manifest.jsonl').write_text(json.dumps(row)+'\n',encoding='utf-8')
            run=[dict(type='run',records=1,options={'--mode':'stt','--split':'development'}),
                 dict(type='record',id='private',source=row['source'],language='ru',split='development',
                      input='',text='центр выплаты',elapsed_ms=1,first_after_load=True,audio_seconds=1,audio_sha256='abc'),
                 dict(type='complete',records=1)]
            result=root/'run.jsonl'
            result.write_text(''.join(json.dumps(r)+'\n' for r in run),encoding='utf-8')
            annotation=dict(id='private',status='fragments_reviewed',audio_sha256='abc',reviewer='human',
                            method='fragment choices',asr_text='unapproved whole draft',
                            reviewed_fragments=[dict(id='term',asr_text='центр выплат')])
            path=root/'annotations.jsonl'
            path.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            report=score(root,result,path)
            group=next(g for g in report['summary'] if g['duration']=='all')
            self.assertEqual(group['scored_records'],0)
            self.assertIsNone(report['records'][0]['reference']['asr_text'])
            self.assertEqual(group['quality']['reviewed_fragments']['fragments'],1)
            self.assertEqual(group['quality']['reviewed_fragments']['matched'],0)
            annotation['audio_sha256']='wrong'
            path.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'Annotation audio/source mismatch'):
                score(root,result,path)

    def test_review_targets_and_output_reviews_are_independent_and_fingerprinted(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            row=dict(id='private',source='sotto-private-history',split='development',language='ru',
                     audio={'sha256':'abc','duration_seconds':10},observed={'raw_text':'не меняй'},
                     reference={'status':'unreviewed','asr_text':None,'punctuated_text':None,'clean_text':None})
            (root/'manifest.jsonl').write_text(json.dumps(row)+'\n',encoding='utf-8')
            run=[dict(type='run',records=1,options={'--mode':'format','--split':'development'}),
                 dict(type='record',id='private',source=row['source'],language='ru',split='development',
                      input='не меняй',text='Меняй.',elapsed_ms=1,first_after_load=True,audio_seconds=10),
                 dict(type='complete',records=1)]
            result=root/'run.jsonl'
            result.write_text(''.join(json.dumps(r)+'\n' for r in run),encoding='utf-8')
            annotation=dict(id='private',status='clean_reviewed',audio_sha256='abc',reviewer='human',
                            method='audio review',clean_text='Не меняй.',asr_text='unapproved draft')
            annotations=root/'annotations.jsonl'
            annotations.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            review=dict(id='private',status='reviewed',reviewer='human',method='output review',
                        run_sha256=hashlib.sha256(result.read_bytes()).hexdigest(),
                        output_sha256=hashlib.sha256('Меняй.'.encode()).hexdigest(),ready_without_edits=False,
                        editing_seconds=5,meaning_errors=[dict(kind='negation',note='lost negation')])
            reviews=root/'reviews.jsonl'
            reviews.write_text(json.dumps(review)+'\n',encoding='utf-8')
            summary=next(g for g in score(root,result,annotations,reviews)['summary'] if g['duration']=='all')
            self.assertEqual(summary['scored_records'],0)
            self.assertEqual(summary['quality']['clean_word_edits']['count'],1)
            self.assertEqual(summary['quality']['editing_time']['seconds_per_audio_minute'],30)
            self.assertEqual(summary['quality']['meaning_errors']['count'],1)
            review['output_sha256']='stale'
            reviews.write_text(json.dumps(review)+'\n',encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'fingerprint mismatch'):
                score(root,result,annotations,reviews)
            review['status']='pending'
            reviews.write_text(json.dumps(review)+'\n',encoding='utf-8')
            annotation['status']='proposed'
            annotations.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            summary=next(g for g in score(root,result,annotations,reviews)['summary'] if g['duration']=='all')
            self.assertIsNone(summary['quality']['meaning_errors']['count'])
            self.assertIsNone(summary['quality']['clean_word_edits']['count'])

    def test_normalization_preserves_numbers_negation_and_yo(self):
        self.assertEqual(normalize(' НЕ ёлка, 3.14 — O’Neil! '), "не ёлка 3 14 o'neil")
        self.assertNotEqual(normalize('ёлка'), normalize('елка'))
        self.assertNotEqual(normalize('нет 12'), normalize('12'))
        self.assertNotEqual(normalize('two'), normalize('2'))
        self.assertEqual(normalize("don't"), "don't")

    def test_alignment_reports_deletion_insertion_and_substitution(self):
        self.assertEqual(alignment(['не','меняй','12'], ['меняй','13']),
                         [('delete','не',None), ('equal','меняй','меняй'),
                          ('substitute','12','13')])

    def test_repeated_words_are_counted_independently(self):
        self.assertEqual(alignment(['no','no'], ['no']),
                         [('delete','no',None), ('equal','no','no')])
        self.assertEqual(alignment([], ['x']), [('insert',None,'x')])
        self.assertEqual(alignment(['x'], []), [('delete','x',None)])

    def test_character_distance_and_meaning_flags(self):
        self.assertEqual(distance('not 12', '12'), 4)
        flags = suspicious('не меняй API 12', 'меняй API 13')
        self.assertEqual(flags['negations_removed'], {'не':1})
        self.assertEqual(flags['numeric_tokens_removed'], {'12':1})
        self.assertEqual(flags['latin_tokens_removed'], {})

    def test_added_negation_has_changed_span_and_context(self):
        flags=suspicious('мы работаем','мы не работаем')
        self.assertEqual(flags['negations_removed'],{})
        self.assertEqual(flags['numeric_changes'],[])
        self.assertEqual(flags['negation_changes'],[{
            'before':'','after':'не','before_context':'мы работаем',
            'after_context':'мы не работаем','diff':[('insert',None,'не')]}])

    def test_moved_negation_is_flagged_without_a_removed_token(self):
        flags=suspicious('не отправляй 3 файла','отправляй не 3 файла')
        self.assertEqual(flags['negations_removed'],{})
        self.assertTrue(flags['negation_changes'])
        self.assertEqual(flags['negation_changes'][0]['before_context'],'не отправляй 3 файла')
        self.assertEqual(flags['negation_changes'][0]['after_context'],'отправляй не 3 файла')
        self.assertTrue(flags['negation_changes'][0]['diff'])

    def test_numeric_substitutions_include_spelled_ru_en_and_digits(self):
        for before,after in (('пять','шесть'),('five','six'),('12','13')):
            with self.subTest(before=before):
                flags=suspicious('send '+before+' files','send '+after+' files')
                self.assertEqual(flags['negation_changes'],[])
                self.assertEqual(flags['numeric_changes'],[{
                    'before':before,'after':after,'before_context':'send '+before+' files',
                    'after_context':'send '+after+' files','diff':[('substitute',before,after)]}])

    def test_case_punctuation_and_word_substrings_do_not_create_critical_flags(self):
        for before,after in (('НЕ меняй ПЯТЬ файлов API12.','не меняй пять файлов, API12!'),
                             ('неточный необычный пятилетка stone','неточным необычно пятилеткам stones')):
            with self.subTest(before=before):
                flags=suspicious(before,after)
                self.assertEqual(flags['negation_changes'],[])
                self.assertEqual(flags['numeric_changes'],[])

    def test_review_prompt_for_added_negation_does_not_adjudicate_meaning(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            source=dict(id='synthetic',source='sotto-private-history',split='development',
                        language='ru',audio=None,observed={'raw_text':'мы работаем'},
                        reference={'asr_text':None,'punctuated_text':None,'status':'unreviewed'})
            (root/'manifest.jsonl').write_text(json.dumps(source)+'\n',encoding='utf-8')
            rows=[dict(type='run',records=1,options={'--mode':'format','--split':'development'}),
                  dict(type='record',id=source['id'],source=source['source'],split=source['split'],language='ru',
                       input='мы работаем',text='мы не работаем',elapsed_ms=1,first_after_load=True,audio_seconds=None),
                  dict(type='complete',records=1)]
            path=root/'run.jsonl'
            path.write_text(''.join(json.dumps(r)+'\n' for r in rows),encoding='utf-8')
            report=score(root,path)
            group=next(g for g in report['summary'] if g['duration']=='all')
            self.assertEqual(group['suspicious_records'],1)
            self.assertEqual(group['scored_records'],0)
            self.assertIsNone(group['quality']['meaning_errors']['count'])

    def test_private_historical_output_is_not_a_reference(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = [dict(id='synthetic', source='sotto-private-history', split='development',
                             language='ru',audio=None,observed={'raw_text':'текст'},
                             reference={'asr_text':None,'punctuated_text':None,'status':'unreviewed'})]
            (root/'manifest.jsonl').write_text(json.dumps(manifest[0])+'\n',encoding='utf-8')
            records = [dict(type='run',records=1,options={'--mode':'format','--split':'development'}),
                       dict(type='record',id='synthetic',source='sotto-private-history',split='development',
                            language='ru',input='текст',text='Текст.',elapsed_ms=1,first_after_load=True,audio_seconds=None),
                       dict(type='complete',records=1)]
            path = root/'result.jsonl'
            def save(rows):
                path.write_text(''.join(json.dumps(r)+'\n' for r in rows),encoding='utf-8')
            save(records)
            report = score(root,path)
            self.assertTrue(all(g['scored_records']==0 and 'normalized' not in g for g in report['summary']))
            save(records[:-1])
            with self.assertRaisesRegex(ValueError,'Incomplete'):
                score(root,path)
            records[1]['split']='holdout'
            save(records)
            with self.assertRaisesRegex(ValueError,'Split mismatch'):
                score(root,path)

    def test_annotations_require_matching_audio_and_review_provenance(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            row=dict(id='private',source='sotto-private-history',split='development',language='ru',
                     audio={'sha256':'abc','duration_seconds':1},observed={'raw_text':'слова'},
                     reference={'status':'unreviewed','asr_text':None,'punctuated_text':None})
            (root/'manifest.jsonl').write_text(json.dumps(row)+'\n',encoding='utf-8')
            run=[dict(type='run',records=1,options={'--mode':'stt','--split':'development'}),
                 dict(type='record',id='private',source=row['source'],language='ru',split='development',
                      input='слова',text='центр выплаты',elapsed_ms=1,first_after_load=True,audio_seconds=1,audio_sha256='abc'),
                 dict(type='complete',records=1)]
            (root/'run.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in run),encoding='utf-8')
            annotation=dict(id='private',status='asr_reviewed',audio_sha256='abc',reviewer='human',
                            method='audio listening',asr_text='центр выплат')
            path=root/'annotation.jsonl'
            path.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            report=score(root,root/'run.jsonl',path)
            summary=next(g for g in report['summary'] if g['duration']=='all')
            self.assertEqual(summary['scored_records'],1)
            self.assertEqual(summary['normalized']['counts'],{'substitute':1})
            annotation['audio_sha256']='wrong'
            path.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'Annotation audio/source mismatch'):
                score(root,root/'run.jsonl',path)
            annotation['audio_sha256']='abc'
            path.write_text(json.dumps(annotation)+'\n',encoding='utf-8')
            run[1]['audio_sha256']='different-recording'
            (root/'run.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in run),encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'ASR audio fingerprint'):
                score(root,root/'run.jsonl',path)


if __name__ == '__main__':
    unittest.main()
