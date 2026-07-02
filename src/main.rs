use clap::{Parser,Subcommand};
use std::io::{BufWriter, Write};

mod error;
use error::AppError;
mod document_intelligence;
use crate::document_intelligence::{Analyzer, TreeChunker};

#[derive(Parser)]
#[command(about)]
struct Cli{
    #[command(subcommand)]
    command: CliCommand,
}
#[derive(Subcommand)]
enum CliCommand{
    /// Extract metadata from input file, write it to output file
    Extract{
        /// Input file (pdf)
        #[arg(short, long)]
        input: String,
        /// Output file (json with Document Intellingence AnalyzeResult)
        #[arg(short, long)]
        output: String,
    },
    /// Dump DocTree to file
    DumpTree{
        /// Input file (AnalyzeResult json)
        #[arg(short, long)]
        input: String,
        /// Output file (DocTree)
        #[arg(short, long)]
        output: String,
    },
    /// Dump raw content of AnalyzeResult into output file
    DumpContent{
        /// Input file (AnalyzeResult json)
        #[arg(short, long)]
        input: String,
        /// Output file (markdown)
        #[arg(short, long)]
        output: String,
    },
    /// Extract chunks from AnalyzeResult, write to output
    Process{
        /// Input file (AnalyzeResult json)
        #[arg(short, long)]
        input: String,
        /// Output file (processed chunks)
        #[arg(short, long)]
        output: String,
    },
}
#[derive(Default)]
pub struct AzureConfig {
    uri: String,
    key: String,
    model: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        CliCommand::Extract { input, output } => extract(input, output).await,
        CliCommand::DumpTree { input, output } => dump_tree_to_file(input, output),
        CliCommand::DumpContent { input, output } => extract_raw_content(input, output),
        CliCommand::Process{ input, output } => process(input, output),
    };
}

async fn extract(input: String, output: String) {
    //get azure config
    let config = get_azure_config();
    //extract markdown
    let mut analyzer = document_intelligence::Analyzer::new(&config);
    match analyzer.send_file_to_analyze(input).await{
        Ok(()) => {
            eprintln!("File sent succesfully to Document Intelligence");
        },
        Err(error) => {
            eprintln!("Error sending file to document intelligence: {}", error);
            return;
        }
    };
    match analyzer.retrieve_analyze_result().await {
        Ok(()) => {
            eprintln!("Analyze result succeeded.");
        },
        Err(error) => {
            eprintln!("Error getting document intelligence results: {}", error);
            return;
        }
    };
    //write result to output file
    let json_result = analyzer.get_raw_json().expect("could not get raw json");
    std::fs::write(output, json_result).expect("Error writing result to output file");
}

fn get_azure_config() -> AzureConfig {
    //get Azure uri & key
    let azure_uri = std::env::var("URI").expect("Azure URI should be set in env");
    let azure_key = std::env::var("KEY").expect("Azure KEY should be set in env");
    let azure_model = std::env::var("MODEL").expect("Azure MODEL should be set in env");
    AzureConfig {
        uri: azure_uri,
        key: azure_key,
        model: azure_model,
    }
}


fn dump_tree_to_file(input: String, output: String) {
    let mut analyzer = document_intelligence::Analyzer::new(&AzureConfig::default());
    //read input file
    let input_str = std::fs::read_to_string(&input).expect("Input file should be readable");
    analyzer.results_from_str(input_str).expect("could not load results from string");
    let file = std::fs::File::create(output).expect("Ouput file shold be writable");
    let mut writer = BufWriter::new(file);
    let tree = analyzer.tree_from_analyze_result().expect("could not parse doc tree");
    write!(writer, "{}", tree).expect("could not write tree to file");
}


fn extract_raw_content(input: String, output: String) {
    let mut analyzer = document_intelligence::Analyzer::new(&AzureConfig::default());
    //read input file
    let input_str = std::fs::read_to_string(&input).expect("Input file should be readable");
    analyzer.results_from_str(input_str).expect("could not load results from string");
    let file = std::fs::File::create(output).expect("Ouput file shold be writable");
    let mut writer = BufWriter::new(file);
    write!(writer, "{}", analyzer.get_raw_content().unwrap()).expect("could not write content to file");
}

fn process(input: String, output: String) {
    let mut analyzer = document_intelligence::Analyzer::new(&AzureConfig::default());
    //read input file
    let input_str = std::fs::read_to_string(&input).expect("Input file should be readable");
    analyzer.results_from_str(input_str).expect("could not load results from string");
    let file = std::fs::File::create(output).expect("Ouput file shold be writable");
    let mut writer = BufWriter::new(file);
    let tree = match analyzer.tree_from_analyze_result() {
        Ok(tree) => tree,
        Err(err) => panic!("Could not generate tree from results: {}", err),
    };
    let mut chunker = TreeChunker::new(800);
    chunker.generate_chunks(&tree);
    let chunks = chunker.chunks();
    for chunk in chunks {
        write!(writer, "{}", chunk).expect("could not write tree to file");
    }
}
