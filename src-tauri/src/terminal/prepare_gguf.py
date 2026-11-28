"""Prepare an independent safetensors training base from one local GGUF. Never fetch model code."""
import shutil
import sys
import tempfile
from pathlib import Path


def prepare(source, output):
    import torch
    from gguf import GGUFReader
    from transformers import AutoModelForCausalLM, AutoTokenizer

    if output.exists():
        raise ValueError('Choose a new output folder; existing files are never replaced.')
    reader = GGUFReader(str(source))
    if 'split.count' in reader.fields or not reader.tensors:
        raise ValueError('Choose a single, unsplit GGUF language checkpoint with weights.')
    required = 2 * sum(int(t.n_elements) for t in reader.tensors)
    if shutil.disk_usage(output.parent).free < required + (64 << 20):
        raise ValueError('Not enough free disk space for the dequantized weights.')
    del reader
    print(f'Preparing a separate checkpoint (approximately {required / 2**30:.2f} GiB of weights).', flush=True)
    # Isolate the selected file: do not pick up config, custom code, adapters or tokenizers
    # from sibling checkpoints in the source folder. All model inputs are local and read-only.
    with tempfile.TemporaryDirectory() as source_dir:
        Path(source_dir, 'source.gguf').symlink_to(source.absolute())
        args = dict(gguf_file='source.gguf', local_files_only=True, trust_remote_code=False)
        tokenizer = AutoTokenizer.from_pretrained(source_dir, **args)
        if not tokenizer.chat_template:
            raise ValueError('No chat template in the GGUF. Use the original chat checkpoint instead.')
        model, info = AutoModelForCausalLM.from_pretrained(source_dir, dtype=torch.float16,
                                                          output_loading_info=True, **args)
        if any(info.get(k) for k in ('missing_keys', 'unexpected_keys', 'mismatched_keys', 'error_msgs')):
            raise ValueError('The GGUF did not map completely to the model architecture. Use the original checkpoint.')
        # Reserve a new folder exclusively. Publish only the finished checkpoint; partial
        # writes are never presented as a trainable base and no existing folder is removed.
        output.mkdir(mode=0o700)
        try:
            with tempfile.TemporaryDirectory(dir=output) as scratch:
                stage = Path(scratch, 'weights')
                model.save_pretrained(stage)
                tokenizer.save_pretrained(stage)
                stage.rename(output / 'checkpoint')
        except BaseException:
            output.rmdir()
            raise
    print(f'Ready. In LoRA training, choose Browse folder and select:\n{output / "checkpoint"}', flush=True)


if __name__ == '__main__':
    try:
        prepare(Path(sys.argv[1]), Path(sys.argv[2]))
    except Exception as error:
        sys.exit(f'Could not prepare these weights: {error}\nYou can also train on the original compatible MLX checkpoint.')
