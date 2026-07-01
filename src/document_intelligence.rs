use std::fmt::Display;
use reqwest::{Client, StatusCode};
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
#[derive(Deserialize, Serialize)]
pub struct AnalyzeOperation {
    pub status: String,
    error: Option<AzureError>,
    #[serde(rename="analyzeResult")]
    pub analyze_result: Option<AnalyzeResult>,
}
#[derive(Deserialize, Serialize)]
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
    content: String,
    figures: Vec<DocumentFigure>,
    pages: Vec<DocumentPage>,
    paragraphs: Vec<DocumentParagraph>,
    tables: Vec<DocumentTable>,
    sections: Vec<DocumentSection>,
}
#[derive(Deserialize, Serialize)]
struct DocumentSection{
    elements: Vec<String>,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize, Clone)]
struct DocumentFigure{
    id: Option<String>,
    caption: Option<DocumentCaption>,
    elements: Option<Vec<String>>,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize, Clone)]
struct DocumentCaption{
    content: String,
    elements: Vec<String>,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize)]
struct DocumentPage{
    #[serde(rename="pageNumber")]
    page_number: u16,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize, Clone)]
struct DocumentTable{
    #[serde(rename="rowCount")]
    row_count: u16,
    #[serde(rename="columnCount")]
    column_count: u16,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize,Clone)]
struct DocumentParagraph{
    content: String,
    role: Option<ParagraphRole>,
    spans: Vec<Span>,
}
#[derive(Deserialize, Serialize, Clone)]
enum ParagraphRole{
    #[serde(rename="pageHeader")]
    PageHeader,
    #[serde(rename="pageFooter")]
    PageFooter,
    #[serde(rename="pageNumber")]
    PageNumber,
    #[serde(rename="title")]
    Title,
    #[serde(rename="sectionHeading")]
    SectionHeading,
    #[serde(rename="footnote")]
    Footnote,
    #[serde(rename="formulaBlock")]
    FormulaBlock,
}

#[derive(Deserialize, Serialize, Clone)]
struct Span{
    offset: usize,
    length: usize,
}

pub struct Analyzer{
    azure_uri: String,
    azure_model: String,
    azure_key: String,
    http_client: Client,
    analyze_handle: Option<DIAnalyzeHandle>,
    analyze_result: Option<AnalyzeResult>,
    utf16_content: Vec<u16>,
}

impl Analyzer{
    pub fn new(config: &AzureConfig) -> Self {
        Self{
            azure_uri: config.uri.clone(), azure_model: config.model.clone(), azure_key: config.key.clone(),
            http_client: Client::new(),
            analyze_handle: None, analyze_result: None,
            utf16_content: Vec::new(),
        }
    }
    fn set_result(&mut self, result: AnalyzeResult) {
        self.utf16_content = result.content.encode_utf16().collect();
        self.analyze_result = Some(result);
    }

    pub async fn send_file_to_analyze(&mut self, input: String) -> Result<(), AppError> {
        //read input and make b64
        let input_content = std::fs::read(input).expect("input file should be readable");
        let b64_input = BASE64_STANDARD.encode(input_content);

        //Azure endpoint format: POST {endpoint}/documentintelligence/documentModels/{modelId}:analyze?_overload=analyzeDocument&api-version=2024-11-30
        let endpoint = format!(
            "{}/documentintelligence/documentModels/{}:analyze?_overload=analyzeDocument&api-version={}&outputContentFormat={}&stringIndexType={}",
            self.azure_uri, self.azure_model, "2024-11-30", "markdown", "utf16CodeUnit");
        let response = self.http_client.post(endpoint)
            .header("Ocp-Apim-Subscription-Key", self.azure_key.clone())
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

        self.analyze_handle = Some(DIAnalyzeHandle{location: location.to_string(), retry});
        Ok(())

    }

    pub async fn retrieve_analyze_result(&mut self) -> Result<(), AppError> {
        let Some(handle) = &self.analyze_handle else {
            return Err(AppError::Azure("No analyze handle, did you call send_to_analyze?".to_string()));
        };
        let mut sleep_secs = handle.retry;
        for i in 0..3 {
            sleep(Duration::from_secs(sleep_secs as u64)).await;
            let response = self.http_client.get(handle.location.clone())
                .header("Ocp-Apim-Subscription-Key", self.azure_key.clone())
                .send().await?;
            let response = response.json::<AnalyzeOperation>().await?;
            match response.status.as_str() {
                "succeeded" => {
                    if let Some(result) = response.analyze_result {
                        self.set_result(result);
                        return Ok(());
                    }else {
                        return Err(AppError::Azure("Operation succeeded but no result fetched".to_string()));
                    }
                },
                "notStarted" | "running" => {
                    sleep_secs *= 2;
                    eprintln!("Cycle {}: operation not started or still running, sleep again for {}s", i, sleep_secs);
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

    fn get_span_text(&self, span: &Span) -> String {
        let content_slice = &self.utf16_content[span.offset..span.offset + span.length];
        String::from_utf16(content_slice).unwrap_or("<!!! Span not found !!!>".to_string())
    }

    pub fn tree_from_analyze_result(&self) -> Result<DocTree, AppError> {
        let Some(analyze) = &self.analyze_result else {
            return Err(AppError::Azure("No analyze rezult available to generate tree".to_string()));
        };
        let mut root = TreeSection{children: Vec::new()};
        //get first section as the root of the tree
        let root_section = analyze.sections.first()
            .ok_or(AppError::Azure("No root section in AnalyzeResult".to_string()))?;
        for elem_str in &root_section.elements {
            let element = self.element_from_str(elem_str)?;
            root.children.push(element);
        }
        Ok(DocTree{root: TreeElement::Section(root)})
    }

    fn element_from_str(&self, elem_str: &str) -> Result<TreeElement, AppError> {
        let Some(analyze) = &self.analyze_result else {
            return Err(AppError::Azure("No analyze rezult available to generate tree".to_string()));
        };
        //elem_str should be: "/paragraphs/5" or "/tables/4" or "/figures/1" or "/sections/5"
        let splitted: Vec<&str> = elem_str.split('/').collect();
        if splitted.len() != 3 {
            return Err(AppError::Azure(format!("Invalid element selector {}, got {:?}", elem_str, splitted)));
        }
        let elem_type = splitted[1];
        let index: usize = splitted[2].parse().expect("Invalid element index");
        let element = match elem_type {
            "paragraphs" => {
                let Some(paragraph) = analyze.paragraphs.get(index) else {
                    return Err(AppError::Azure(format!("Paragraph with id {} not found", index)));
                };
                let mut content = String::new();
                for span in &paragraph.spans {
                    content.push_str(&self.get_span_text(span));
                }
                let role = paragraph.role.clone();
                TreeElement::Paragraph(TreeParagraph{content, role})
            },
            "tables" => {
                let Some(table) = analyze.tables.get(index) else {
                    return Err(AppError::Azure(format!("Table with id {} not found", index)));
                };
                let mut content = String::new();
                for span in &table.spans {
                    content.push_str(&self.get_span_text(span));
                }
                TreeElement::Table(content)
            },
            "figures" => {
                let Some(figure) = analyze.figures.get(index) else {
                    return Err(AppError::Azure(format!("Figure with id {} not found", index)));
                };
                let mut content = String::new();
                for span in &figure.spans {
                    content.push_str(&self.get_span_text(span));
                }
                TreeElement::Figure(content)
            },
            "sections" => {
                let Some(section) = analyze.sections.get(index) else {
                    return Err(AppError::Azure(format!("Section with id {} not found", index)));
                };
                TreeElement::Section(self.node_from_section(section))
            },
            _ => return Err(AppError::Azure(format!("Invalid element selector {}", elem_str))),

        };
        Ok(element)
    }

    fn node_from_section(&self, section: &DocumentSection) -> TreeSection {
        let mut node = TreeSection{ children: Vec::new() };
        for element_str in &section.elements {
            if let Ok(element) = self.element_from_str(element_str) {
                node.children.push(element);
            }
        }
        node
    }

    pub fn get_raw_json(&self) -> Result<String, AppError> {
        let Some(analyze) = &self.analyze_result else {
            return Err(AppError::Azure("No analyze rezult available to generate tree".to_string()));
        };
        Ok(serde_json::to_string_pretty(&analyze.content)?)
    }
    pub fn get_raw_content(&self) -> Result<String, AppError> {
        let Some(analyze) = &self.analyze_result else {
            return Err(AppError::Azure("No analyze rezult available".to_string()));
        };
        Ok(analyze.content.clone())
    }
    pub fn results_from_str(&mut self, input_str: String) -> Result<(), AppError> {
        self.set_result(serde_json::from_str(&input_str)?);
        Ok(())
    }


}

enum TreeElement{
    Section(TreeSection),
    Paragraph(TreeParagraph),
    Table(String),
    Figure(String),
}
pub struct DocTree{
    root: TreeElement,
}

struct TreeSection{
    children: Vec<TreeElement>,
}

struct TreeParagraph{
    content: String,
    role: Option<ParagraphRole>,
}

impl Display for DocTree{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.root) 
    }
}

impl Display for TreeElement{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self{
            TreeElement::Paragraph(paragraph) => {
                write!(f, "<PARAGRAPH>\n{}\n</PARAGRAPH>\n", paragraph.content)
            },
            TreeElement::Table(table) => {
                writeln!(f, "<TABLE>\n{}\n</TABLE>\n", table)
            },
            TreeElement::Figure(figure) => {
                write!(f, "<FIGURE>\n{}\n</FIGURE>\n", figure)
            },
            TreeElement::Section(node) => {
                for child in &node.children {
                    write!(f, "{}", child)?;
                }
                Ok(())
            },
        }
    }
}

impl TreeElement{
    pub fn len(&self) -> usize {
        match self {
            TreeElement::Section(section) => {
                let mut size = 0;
                for e in &section.children {
                    size += e.len();
                }
                size
            },
            TreeElement::Paragraph(paragraph) => paragraph.content.len(),
            TreeElement::Table(table) => table.len(),
            TreeElement::Figure(figure) => figure.len(),
        }
    }
}

pub struct Chunks{
    chunk_size: usize,
    items: Vec<Chunk>,
}

impl Display for Chunks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for item in &self.items { write!(f, "{item}")?; }
        Ok(())
    }
}
impl Chunks{
    pub fn new(size: usize) -> Self {
        Chunks{chunk_size: size, items: Vec::new()}
    }
    pub fn push(&mut self, item: Chunk){
        self.items.push(item);
    }
}

#[derive(Default)]
pub struct Chunk{
    text: String,
}
impl Display for Chunk{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\nCHUNK--->\n{}\n<---CHUNK\n", &self.text)
    }
}
impl Chunk{
    pub fn len(&self) -> usize {
        self.text.len()
    }
    fn add(&mut self, elem: &TreeElement) {
        //TODO: do a better add
        self.text.push_str(&format!("{}", elem));
    }
}

impl DocTree {
    pub fn generate_chunks(&self, chunk_size: usize) -> Result<Chunks, AppError> {
        let mut chunks = Chunks::new(chunk_size);
        let mut current = Chunk::default();
        
        // recursively chunk each element
        Self::chunk_element(&self.root, &mut current, &mut chunks);
        // current might have last chunks, push it to chunks
        chunks.push(current);

        Ok(chunks)
    }

    fn chunk_element(element: &TreeElement, current: &mut Chunk, chunks: &mut Chunks) {
        // 1. if sizeof element + sizeof current <= chunk_size => add element to current, return
        if element.len() + current.len() <= chunks.chunk_size {
            current.add(element);
            return;
        }
        // 2. push and empty current to chunks
        chunks.push(std::mem::take(current));
        match element {
            // 3. if simple element add element to chunk, return
            TreeElement::Paragraph(_) 
                | TreeElement::Table(_) 
                | TreeElement::Figure(_) => current.add(element),
            // 4. if section recurse for each child
            TreeElement::Section(section) => {
                for child in &section.children {
                    Self::chunk_element(child, current, chunks);
                }
            },
        };
    }
}

