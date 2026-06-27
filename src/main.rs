use reqwest::{Client, StatusCode};
use base64::prelude::*;
use serde::{Serialize, Deserialize};
use tokio::time::{Duration, sleep};
use clap::{Parser,Subcommand};

mod error;
use error::AppError;

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
    }
}

struct AzureConfig {
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
    };
}

async fn extract(input: String, output: String) {
    //get azure config
    let config = get_azure_config();
    //extract markdown
    let analyze = match send_file_to_analyze(input, &config).await{
        Ok(markdown) => markdown,
        Err(error) => {
            eprintln!("Error sending file to document intelligence: {}", error);
            return;
        }
    };
    let analyze_result = match get_analyze_result(analyze, &config).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Error getting document intelligence results: {}", error);
            return;
        }
    };
    //write result to output file
    match write_to_output(output, analyze_result){
        Ok(()) => {},
        Err(error) => {
            eprintln!("Error writing result to output file: {}", error);
        },
    };
}

fn write_to_output(output: String, result: AnalyzeResult) -> Result<(), AppError> {
    //serialize as json
    let json_result = serde_json::to_string(&result)?;
    std::fs::write(output, json_result)?;
    println!("Done, output file written. Bye!");
    Ok(())
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

#[derive(Serialize)]
struct DIRequestBody{
    #[serde(rename="base64Source")]
    base64_source: String,
}
struct DIAnalyzeHandle {
    location: String,
    retry: u32,
}
#[derive(Deserialize)]
struct AzureResponse {
    status: String,
    error: AzureError,
    #[serde(rename="analyzeResult")]
    analyze_result: AnalyzeResult,
}
#[derive(Deserialize)]
struct AzureError{
    code: String,
    message: String,
}
#[derive(Deserialize, Serialize)]
struct AnalyzeResult{
    content: String,
    pages: Vec<DocumentPage>,
    tables: Vec<DocumentTable>,
}
#[derive(Deserialize, Serialize)]
struct DocumentPage{
    #[serde(rename="pageNumber")]
    page_number: u16,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize)]
struct DocumentTable{
    #[serde(rename="rowCount")]
    row_count: u16,
    #[serde(rename="columnCount")]
    column_count: u16,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize)]
struct Span{
    offset: u32,
    length: u32,
}

async fn send_file_to_analyze(input: String, azure: &AzureConfig) -> Result<DIAnalyzeHandle, AppError> {
    //read input and make b64
    let input_content = std::fs::read(input).expect("input file should be readable");
    let b64_input = BASE64_STANDARD.encode(input_content);

    //Azure endpoint format: POST {endpoint}/documentintelligence/documentModels/{modelId}:analyze?_overload=analyzeDocument&api-version=2024-11-30
    let endpoint = format!(
        "{}/documentintelligence/documentModels/{}:analyze?_overload=analyzeDocument&api-version=2024-11-30&outputContentFormat=markdown",
        azure.uri, azure.model);
    let response = azure.client.post(endpoint)
        .header("Ocp-Apim-Subscription-Key", azure.key.clone())
        .json(&DIRequestBody{
            base64_source: b64_input,
        })
        .send().await?;

    if response.status() != StatusCode::ACCEPTED {
        return Err(AppError::Azure(format!("unexpected response status {}", response.status())));
    }
    //After a succesfull send (ACCEPTED) get Headers
    // Operation-Location: string
    // Retry-After: integer
    let location = response.headers()
        .get("Operation-Location")
        .ok_or(AppError::Azure("Operation-Location header missing!".to_string()))?
        .to_str().map_err(|e| AppError::Other(format!("Malformed Operation-Location header: {}", e)))?;
    let retry = match response.headers().get("Retry-After"){
        Some(retry_header) => {
            let retry_str = retry_header.to_str()
                .map_err(|e| AppError::Other(format!("Malformed Retry-After header: {}", e)))?;
            retry_str.parse::<u32>()
                .map_err(|e| AppError::Other(format!("Malformed Retry-After header: {}", e)))?
        },
        None => 2, //use a default of 2 seconds
    };
    
    Ok(DIAnalyzeHandle{location: location.to_string(), retry})

}

async fn get_analyze_result(handle: DIAnalyzeHandle, azure: &AzureConfig) -> Result<AnalyzeResult, AppError> {
    for _i in 0..3 {
        sleep(Duration::from_secs(handle.retry as u64)).await;
        let response = azure.client.get(handle.location.clone())
            .header("Ocp-Apim-Subscription-Key", azure.key.clone())
            .send().await?;
        let response = response.json::<AzureResponse>().await?;
        if response.status == "succeeded" {
            return Ok(response.analyze_result);
        } else {
            eprintln!("Azure error while fetching results: code {}, message: {}",
                response.error.code, response.error.message);
        }
    }
    Err(AppError::Azure("could not get analyze results".to_string()))
}
