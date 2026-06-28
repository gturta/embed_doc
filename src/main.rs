use reqwest::Client;
use clap::{Parser,Subcommand};

mod error;
use error::AppError;
mod document_intelligence;
use document_intelligence::AnalyzeResult;

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
        /// Input file
        #[arg(short, long)]
        input: String,
        /// Output file
        #[arg(short, long)]
        output: String,
    },
    /// Process Document Intelligence result
    Process{
        /// Input file
        #[arg(short, long)]
        input: String,
    },
}

pub struct AzureConfig {
    uri: String,
    key: String,
    model: String,
    client: Client,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        CliCommand::Extract { input, output } => extract(input, output).await,
        CliCommand::Process { input } => process(input),
    };
}

async fn extract(input: String, output: String) {
    //get azure config
    let config = get_azure_config();
    //extract markdown
    let analyze = match document_intelligence::send_file_to_analyze(input, &config).await{
        Ok(markdown) => {
            eprintln!("File sent succesfully to Document Intelligence");
            markdown
        },
        Err(error) => {
            eprintln!("Error sending file to document intelligence: {}", error);
            return;
        }
    };
    let analyze_result = match document_intelligence::get_analyze_result(analyze, &config).await {
        Ok(result) => {
            eprintln!("Analyze result succeeded.");
            result
        },
        Err(error) => {
            eprintln!("Error getting document intelligence results: {}", error);
            return;
        }
    };
    //write result to output file
    match write_to_output(output, analyze_result){
        Ok(()) => {
            eprintln!("Analyze result written to file.");
        },
        Err(error) => {
            eprintln!("Error writing result to output file: {}", error);
        },
    };
}

fn write_to_output(output: String, result: AnalyzeResult) -> Result<(), AppError> {
    //serialize as json
    let json_result = serde_json::to_string_pretty(&result)?;
    Ok(std::fs::write(output, json_result)?)
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
        client: Client::new(),
    }
}


fn process(input: String) {
    //read input file
    let input_str = std::fs::read_to_string(&input).expect("Input file should be readable");
    let result: AnalyzeResult = serde_json::from_str(&input_str).expect("Input file should contain a AnalyzeResult instance");
    //write markdown
    std::fs::write(format!("{}.md", &input), result.content).expect("could not write md");
}

