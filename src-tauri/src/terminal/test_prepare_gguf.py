"""Offline conversion contract: generate a tiny real quantized Llama chat checkpoint."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from prepare_gguf import prepare


class Conversion(unittest.TestCase):
    def test_quantized_checkpoint_and_no_clobber(self):
        import gguf
        import numpy as np
        from safetensors.numpy import load_file
        from transformers import AutoTokenizer

        with tempfile.TemporaryDirectory() as tmp:
            source, output = Path(tmp, 'tiny.gguf'), Path(tmp, 'prepared')
            writer = gguf.GGUFWriter(source, 'llama')
            for field, value in [('name', 'tiny'), ('embedding_length', 32), ('context_length', 64),
                                 ('block_count', 1), ('feed_forward_length', 64), ('head_count', 2),
                                 ('head_count_kv', 2), ('layer_norm_rms_eps', 1e-5), ('tokenizer_model', 'llama')]:
                getattr(writer, 'add_' + field)(value)
            tokens = ['<unk>', '<s>', '</s>', '▁'] + list('abcdefghijklmnopqrstuvwxyz') + ['.', '?']
            writer.add_token_list(tokens)
            writer.add_token_scores([0.0] * len(tokens))
            writer.add_token_types([2, 3, 3] + [1] * (len(tokens) - 3))
            writer.add_unk_token_id(0)
            writer.add_bos_token_id(1)
            writer.add_eos_token_id(2)
            writer.add_chat_template('{% for m in messages %}{{ m.content }}{% endfor %}')
            embedding = np.ones((32, 32), dtype=np.float32)
            quant = gguf.GGMLQuantizationType.Q8_0
            writer.add_tensor('token_embd.weight', gguf.quantize(embedding, quant), raw_dtype=quant)
            for name, shape in [('output', (32, 32)), ('output_norm', (32,)), ('blk.0.attn_norm', (32,)),
                                ('blk.0.ffn_norm', (32,)), ('blk.0.ffn_gate', (64, 32)), ('blk.0.ffn_up', (64, 32)),
                                ('blk.0.ffn_down', (32, 64))] + [('blk.0.attn_' + k, (32, 32)) for k in ('q', 'k', 'v', 'output')]:
                # Unit normalization scales, small projections: all-ones linear layers
                # overflow the half-precision gated MLP even before conversion.
                writer.add_tensor(name + '.weight', np.full(shape, 1 if len(shape) == 1 else 0.01, dtype=np.float32))
            writer.write_header_to_file()
            writer.write_kv_data_to_file()
            writer.write_tensors_to_file()
            writer.close()
            digest = hashlib.sha256(source.read_bytes()).digest()
            prepare(source, output)
            saved = output / 'checkpoint'
            weights = load_file(next(saved.glob('*.safetensors')))
            np.testing.assert_allclose(weights['model.embed_tokens.weight'], embedding, atol=.001)
            tok = AutoTokenizer.from_pretrained(saved, local_files_only=True)
            self.assertEqual(tok.apply_chat_template([{'role':'user','content':'hi'}], tokenize=False), 'hi')
            # The trainer and converter use independent environments. When provided, check
            # the actual MLX consumer, not just the safetensors container.
            if runtime := os.environ.get('DIAL_TEST_MLX_PYTHON'):
                subprocess.run([runtime, '-I', '-c', '''
import sys
import mlx.core as mx
from mlx_lm import load
model, tokenizer = load(sys.argv[1])
ids = tokenizer.encode('hi')
assert ids and all(0 <= token < 32 for token in ids), ids
logits = model(mx.array([ids]))
assert logits.shape == (1, len(ids), 32) and mx.all(mx.isfinite(logits)).item(), logits.shape
''', str(saved)], check=True)
            with self.assertRaisesRegex(ValueError, 'never replaced'):
                prepare(source, output)
            self.assertEqual(hashlib.sha256(source.read_bytes()).digest(), digest)


if __name__ == '__main__':
    unittest.main()
