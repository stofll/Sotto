import unittest
from speech_eval import alignment, normalize
from speech_eval_quality import aggregate_quality, record_quality


class QualityMetricsTests(unittest.TestCase):
    def test_identical_reviewed_fragments_need_distinct_output_mentions(self):
        reference={'status':'fragments_reviewed','asr_text':None,
                   'reviewed_fragments':[{'id':'first','asr_text':'harness'},
                                         {'id':'second','asr_text':'harness'}]}
        one=self.quality('one harness',reference)
        two=self.quality('harness and harness',reference)
        self.assertEqual(one['reviewed_fragments']['matched'],1)
        self.assertEqual(two['reviewed_fragments']['matched'],2)

    def test_unreviewed_fragment_drafts_are_not_scored(self):
        reference={'status':'proposed','asr_text':None,
                   'reviewed_fragments':[{'id':'draft','asr_text':'not approved'}]}
        result=self.quality('not approved',reference)
        self.assertNotIn('reviewed_fragments',result)
        self.assertIsNone(aggregate_quality([result])['reviewed_fragments']['accuracy'])

    def test_fragment_targets_preserve_spoken_repetition_but_allow_reviewed_cleanup(self):
        reference = {'status':'fragments_reviewed','asr_text':None,
                     'reviewed_fragments':[{'id':'repeat','asr_text':'where it where it remains',
                                            'clean_text':'where it remains',
                                            'clean_exclusions':['where it where it']}]}
        record = {'text':'before where it remains after','input':'','audio_seconds':1}
        raw = record_quality(record,reference,normalize,alignment,mode='stt')
        formatted = record_quality(record,reference,normalize,alignment,mode='format')
        self.assertEqual(raw['reviewed_fragments']['matched'],0)
        self.assertEqual(formatted['reviewed_fragments']['matched'],1)
        self.assertNotIn('clean_word_edits',formatted)
        self.assertIsNone(aggregate_quality([formatted])['ready_without_edits']['rate'])
        record['text']='before where it where it remains after'
        repeated=record_quality(record,reference,normalize,alignment,mode='format')
        self.assertEqual(repeated['reviewed_fragments']['matched'],0)

    def test_fragment_matching_counts_whole_phrases_and_rejects_duplicate_ids(self):
        reference = {'status':'fragments_reviewed','asr_text':None,
                     'reviewed_fragments':[{'id':'term','asr_text':'Pi'}]}
        record = {'text':'Python','input':'','audio_seconds':1}
        result = record_quality(record,reference,normalize,alignment,mode='stt')
        self.assertEqual(result['reviewed_fragments']['matched'],0)
        reference['reviewed_fragments'] *= 2
        with self.assertRaises(ValueError):
            record_quality(record,reference,normalize,alignment,mode='stt')

    def test_nested_term_spellings_do_not_double_count_one_mention(self):
        reference = {'status':'asr_reviewed','asr_text':'Open AI',
                     'terms':[{'text':'Open AI','accepted':['Open AI','AI']}]}
        result = record_quality({'text':'Open AI','input':'','audio_seconds':1},reference,normalize,alignment)
        self.assertEqual(result['terms'],{'mentions':1,'correct':1,'extra':0})

    def test_term_variants_require_a_list_of_nonempty_spellings(self):
        for variants in ('API',[],['']):
            with self.assertRaises(ValueError):
                self.quality('API',dict(status='asr_reviewed',asr_text='API',terms=[dict(text='API',accepted=variants)]))

    def quality(self, text, reference, review=None, input_text='alpha beta'):
        return record_quality(dict(text=text,input=input_text,audio_seconds=10), reference, normalize, alignment, review)

    def test_unreviewed_clean_text_and_absent_judgement_are_not_zero_errors(self):
        result=self.quality('alpha beta',dict(asr_text='alpha beta',clean_text='alpha beta',status='asr_reviewed'))
        totals=aggregate_quality([result])
        self.assertIsNone(totals['clean_word_edits']['per_100_words'])
        self.assertEqual(totals['clean_word_edits']['reviewed_records'],0)
        self.assertIsNone(totals['ready_without_edits']['rate'])
        self.assertIsNone(totals['meaning_errors']['count'])

    def test_clean_edit_proxy_and_terms_use_explicit_reviewed_targets(self):
        reference=dict(asr_text='use API API now',clean_text='use API API now',status='reviewed',terms=[dict(text='API',accepted=['API'])])
        result=self.quality('use API now',reference)
        totals=aggregate_quality([result])
        self.assertEqual(totals['clean_word_edits']['count'],1)
        self.assertEqual(totals['clean_word_edits']['per_100_words'],25)
        self.assertEqual(totals['terms']['correct'],1)
        self.assertEqual(totals['terms']['mentions'],2)
        self.assertEqual(totals['terms']['accuracy'],.5)

    def test_human_metrics_have_independent_coverage(self):
        review=dict(ready_without_edits=False,editing_seconds=5,mandatory_word_edits=2,
                    punctuation_edits=1,paragraph_judgement='split',meaning_errors=[dict(kind='number',note='changed quantity')],postprocess_damage=[])
        result=self.quality('alpha beta',dict(asr_text=None,status='unreviewed'),review)
        totals=aggregate_quality([result, self.quality('no gold',dict(asr_text=None,status='unreviewed'))])
        self.assertEqual(totals['ready_without_edits']['reviewed_records'],1)
        self.assertEqual(totals['ready_without_edits']['rate'],0)
        self.assertEqual(totals['editing_time']['seconds_per_audio_minute'],30)
        self.assertEqual(totals['meaning_errors']['count'],1)
        self.assertEqual(totals['postprocess_damage']['count'],0)

    def test_correct_input_damage_is_separate_from_ordinary_asr_errors(self):
        result=self.quality('35',dict(asr_text='35 мм',status='publisher_provided'),input_text='35 мм')
        totals=aggregate_quality([result])
        self.assertEqual(totals['correct_input_damage']['word_edits'],1)
        self.assertEqual(totals['correct_input_damage']['eligible_records'],1)
        other=self.quality('35',dict(asr_text='35 мм',status='publisher_provided'),input_text='recognition error')
        self.assertEqual(aggregate_quality([other])['correct_input_damage']['eligible_records'],0)

    def test_approved_restart_cleanup_is_not_counted_as_damage_to_correct_input(self):
        reference={'status':'reviewed','reviewed_targets':['asr_text','clean_text'],
                   'asr_text':'where it where it remains 12','clean_text':'where it remains 12'}
        good=self.quality('where it remains 12',reference,input_text=reference['asr_text'])
        self.assertEqual(good['correct_input_damage'],0)
        unchanged=self.quality(reference['asr_text'],reference,input_text=reference['asr_text'])
        self.assertEqual(unchanged['correct_input_damage'],0)
        bad=self.quality('where it remains',reference,input_text=reference['asr_text'])
        self.assertEqual(bad['correct_input_damage'],1)

    def test_manual_time_rejects_nonfinite_and_negative_values(self):
        for value in [-1,float('nan'),float('inf')]:
            with self.assertRaises(ValueError):
                self.quality('text',dict(asr_text=None,status='unreviewed'),dict(editing_seconds=value))


if __name__=='__main__':
    unittest.main()
