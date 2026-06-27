# Document embedder

This is a small utility that can generate embeddings from a pdf document.

What it does:
- Read input file
- Call Azure Document Intelligence to analyze document
- Get results an write them into the output file (for the moment)

Running:
 > embed_doc <input_file> <output_file>

Configuration:

Requires the env to have defined:
 - URI: endpoint of azure project
 - KEY: auth key for the project
 - MODEL: the layout model to use

