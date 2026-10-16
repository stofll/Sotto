import copy
import unittest
from compare_speech_eval import compare


def report(model,errors):
    return dict(manifest_sha256='fixed',annotations_sha256=None,normalization='fixed',
                metadata=dict(options={'--mode':'stt','--model':model,'--split':'development'},compute='cpu',threads=8,cpu='test',os='windows'),
                records=[dict(id=str(i),group_id=str(i),source='public',language='ru',split='development',
                              audio_seconds=10,audio_sha256=str(i),reference={'asr_text':'same reference'},
                              normalized_words=10,normalized_counts={'substitute':n},elapsed_ms=100+i,
                              normalized_chars=50,normalized_char_errors=n*2,
                              first_after_load=i==0,quality={}) for i,n in enumerate(errors)])


class PairedTests(unittest.TestCase):
    def test_word_error_delta_and_pairs_share_one_denominator(self):
        result=compare(report('parakeet-ultra',[2,0,3]),report('candidate',[1,1,3]),bootstrap_samples=100)
        group=next(g for g in result['groups'] if g['duration']=='all')
        self.assertEqual(group['wer']['reference_words'],30)
        self.assertAlmostEqual(group['wer']['delta'],0)
        self.assertEqual(group['wer']['improved_records'],1)
        self.assertEqual(group['wer']['regressed_records'],1)
        self.assertEqual(group['wer']['unchanged_records'],1)
        self.assertEqual(group['wer']['clusters'],3)
        self.assertLess(group['wer']['delta_ci95'][0],0)
        self.assertGreater(group['wer']['delta_ci95'][1],0)
        self.assertEqual(group['cer']['reference_chars'],150)
        self.assertEqual(group['cer']['baseline_errors'],10)
        self.assertEqual(group['cer']['delta'],0)

    def test_unpaired_human_metrics_do_not_fake_progress(self):
        left,right=report('parakeet-ultra',[0,0]),report('candidate',[0,0])
        left['records'][0]['quality']['ready_without_edits']=False
        right['records'][1]['quality']['ready_without_edits']=True
        result=compare(left,right,bootstrap_samples=10)
        group=next(g for g in result['groups'] if g['duration']=='all')
        self.assertEqual(group['quality']['ready_without_edits']['paired_records'],0)
        self.assertIsNone(group['quality']['ready_without_edits']['delta'])
        self.assertIsNone(group['quality']['new_confirmed_meaning_error_records'])

    def test_replacing_one_confirmed_error_with_another_is_still_a_new_error(self):
        left,right=report('parakeet-ultra',[0]),report('candidate',[0])
        left['records'][0]['quality']['meaning_errors']=[{'kind':'number','note':'changed quantity'}]
        right['records'][0]['quality']['meaning_errors']=[{'kind':'negation','note':'removed not'}]
        group=next(g for g in compare(left,right,bootstrap_samples=10)['groups'] if g['duration']=='all')
        self.assertEqual(group['quality']['meaning_errors']['delta'],0)
        self.assertEqual(group['quality']['new_confirmed_meaning_error_records'],['0'])

    def test_refuses_changed_sample_labels_or_timing_hardware(self):
        left=report('parakeet-ultra',[0,0])
        for change in ('sample','labels','hardware','audio'):
            right=copy.deepcopy(left)
            if change=='sample': right['records'].pop()
            if change=='labels': right['records'][0]['reference']['asr_text']='different'
            if change=='hardware': right['metadata']['threads']=4
            if change=='audio': right['records'][0]['audio_sha256']='different'
            with self.assertRaises(ValueError):
                compare(left,right)

    def test_formatter_comparison_refuses_corrected_or_missing_input(self):
        left=report('formatter',[2,1])
        left['metadata']['options']['--mode']='format'
        for record in left['records']:
            record['input']='проверь рабочий нут'
        for changed_input in ('проверь рабочий ноут',None):
            with self.subTest(input=changed_input):
                right=copy.deepcopy(left)
                right['records'][0]['normalized_counts']={}
                if changed_input is None:
                    right['records'][0].pop('input')
                else:
                    right['records'][0]['input']=changed_input
                with self.assertRaisesRegex(ValueError,'input mismatch'):
                    compare(left,right,bootstrap_samples=10)

    def test_formatter_compares_equal_inputs_from_different_input_files(self):
        left,right=report('formatter',[2,1]),report('candidate',[1,1])
        for side,fingerprint in ((left,'baseline-file'),(right,'candidate-file')):
            side['metadata']['options']['--mode']='format'
            side['metadata']['input_sha256']=fingerprint
            for record in side['records']:
                record['input']='проверь рабочий нут'
        group=next(g for g in compare(left,right,bootstrap_samples=10)['groups'] if g['duration']=='all')
        self.assertEqual(group['wer']['candidate_errors'],2)
        self.assertEqual(group['wer']['improved_records'],1)

    def test_stt_does_not_compare_unrelated_text_inputs(self):
        left,right=report('parakeet-ultra',[2,1]),report('candidate',[1,1])
        for record in left['records']:
            record['input']='исторический черновик'
        for record in right['records']:
            record['input']='другой исторический черновик'
        group=next(g for g in compare(left,right,bootstrap_samples=10)['groups'] if g['duration']=='all')
        self.assertEqual(group['wer']['improved_records'],1)


if __name__=='__main__':
    unittest.main()
