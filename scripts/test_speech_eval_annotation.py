import unittest
from speech_eval_annotation import proposals


def run(model,text,hints='empty'):
    return {'metadata':{'options':{'--model':model},'hints_mode':hints},
            'records':[{'id':'clip','text':text,'elapsed_ms':1}]}


class AnnotationTests(unittest.TestCase):
    def test_agreement_is_a_proposal_and_hint_variants_are_not_independent_votes(self):
        manifest=[{'id':'clip','source':'sotto-private-history','language':'ru','audio':{'sha256':'abc','path':'audio.wav','duration_seconds':3},
                   'reference':{'status':'unreviewed'}}]
        rows=proposals(manifest,[run('parakeet-ultra','Не меняй 12.'),run('qwen3-asr-0.6b','не меняй 12'),run('qwen3-asr-0.6b','не меняй 12','config')])
        self.assertEqual(rows[0]['status'],'proposed')
        self.assertEqual(rows[0]['independent_models'],2)
        self.assertTrue(rows[0]['unanimous_normalized'])
        self.assertIsNone(rows[0]['reviewer'])

    def test_reviewed_speech_is_skipped_and_ultra_is_required(self):
        manifest=[{'id':'clip','source':'sotto-private-history','language':'ru',
                   'audio':{'sha256':'abc','path':'audio.wav','duration_seconds':3},'reference':{'status':'asr_reviewed'}}]
        self.assertEqual(proposals(manifest,[run('parakeet-ultra','text')]),[])
        manifest[0]['reference']={'status':'fragments_reviewed','reviewed_fragments':[{'id':'term','asr_text':'text'}]}
        self.assertEqual(proposals(manifest,[run('parakeet-ultra','text')]),[])
        manifest[0]['reference']['status']='proposed'
        self.assertEqual(len(proposals(manifest,[run('parakeet-ultra','text')])),1)
        with self.assertRaisesRegex(ValueError,'Ultra'):
            proposals(manifest,[run('other','text')])


if __name__=='__main__':
    unittest.main()
