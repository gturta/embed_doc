use reqwest::StatusCode;
use base64::prelude::*;
use serde::{Serialize, Deserialize};
use tokio::time::{Duration, sleep};
use crate::error::AppError;
use crate::AzureConfig;

#[derive(Serialize)]
struct DIRequestBody{
    #[serde(rename="base64Source")]
    base64_source: String,
}
pub struct DIAnalyzeHandle {
    location: String,
    retry: u32,
}
#[derive(Deserialize)]
struct AzureResponse {
    status: String,
    error: Option<AzureError>,
    #[serde(rename="analyzeResult")]
    analyze_result: Option<AnalyzeResult>,
}
#[derive(Deserialize)]
struct AzureError{
    code: String,
    message: String,
}
#[derive(Deserialize, Serialize)]
pub struct AnalyzeResult{
    #[serde(rename="contentFormat")]
    content_format: String,
    #[serde(rename="stringIndexType")]
    string_index_type: String,
    pub content: String,
    pub pages: Vec<DocumentPage>,
    pub paragraphs: Vec<DocumentParagraph>,
    pub tables: Vec<DocumentTable>,
}
#[derive(Deserialize, Serialize)]
pub struct DocumentPage{
    #[serde(rename="pageNumber")]
    page_number: u16,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize)]
pub struct DocumentTable{
    #[serde(rename="rowCount")]
    row_count: u16,
    #[serde(rename="columnCount")]
    column_count: u16,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize)]
pub struct DocumentParagraph{
    content: String,
    role: Option<String>,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize)]
struct Span{
    offset: u32,
    length: u32,
}

pub async fn send_file_to_analyze(input: String, azure: &AzureConfig) -> Result<DIAnalyzeHandle, AppError> {
    //read input and make b64
    let input_content = std::fs::read(input).expect("input file should be readable");
    let b64_input = BASE64_STANDARD.encode(input_content);

    //Azure endpoint format: POST {endpoint}/documentintelligence/documentModels/{modelId}:analyze?_overload=analyzeDocument&api-version=2024-11-30
    let endpoint = format!(
        "{}/documentintelligence/documentModels/{}:analyze?_overload=analyzeDocument&api-version={}&outputContentFormat={}&stringIndexType={}",
        azure.uri, azure.model, "2024-11-30", "markdown", "utf16CodeUnit");
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

pub async fn get_analyze_result(handle: DIAnalyzeHandle, azure: &AzureConfig) -> Result<AnalyzeResult, AppError> {
    for i in 0..3 {
        sleep(Duration::from_secs(handle.retry as u64)).await;
        let response = azure.client.get(handle.location.clone())
            .header("Ocp-Apim-Subscription-Key", azure.key.clone())
            .send().await?;
        let response = response.json::<AzureResponse>().await?;
        match response.status.as_str() {
            "succeeded" => {
                if let Some(result) = response.analyze_result {
                    return Ok(result);
                }else {
                    return Err(AppError::Azure("Operation succeeded but no result fetched".to_string()));
                }
            },
            "notStarted" | "running" => {
                eprintln!("Cycle {}: operation not started or still running", i);
            },
            "failed" => {
                if let Some(error) = response.error {
                    return Err(AppError::Azure(format!("Operation failed with code {}, message: {}",
                                error.code, error.message)));
                } else {
                    return Err(AppError::Azure("Operation failed with unknown error".to_string()));
                }
            },
            "cancelled" | "skipped" => {
                return Err(AppError::Azure("Operation cancelled or skipped".to_string()));
            },
            _ => return Err(AppError::Azure("Unknown operation status".to_string())),
        };
    }
    Err(AppError::Azure("could not get analyze results".to_string()))
}
