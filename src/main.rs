use reqwest::Client;
use clap::{Parser,Subcommand};
use std::io::{BufWriter, Write};
use markdown::{to_mdast, ParseOptions, mdast::Node};

mod error;
use error::AppError;
mod document_intelligence;
use crate::document_intelligence::{AnalyzeResult, AnalyzeOperation};

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
        /// Input file (AnalyzeOperation json)
        #[arg(short, long)]
        input: String,
        /// Output file (chunked markdown)
        #[arg(short, long)]
        output: String,
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
        CliCommand::Process { input, output } => process(input, output),
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


fn process(input: String, output: String) {
    //read input file
    let input_str = std::fs::read_to_string(&input).expect("Input file should be readable");
    let op: AnalyzeOperation = serde_json::from_str(&input_str).expect("Input file should contain a AnalyzeOperation instance");
    //write markdown
    if let Some(result) = op.analyze_result {
        let tree = to_mdast(&result.content, &ParseOptions::default())
            .expect("unable to parse markdown contents into AST");
        let file = std::fs::File::create(output).expect("Ouput file shold be writable");
        let mut writer = BufWriter::new(file);
        //debug_print_tree(&tree, 0);
        traverse_ast(&tree, 0, &mut writer);
    }
}

fn traverse_ast(node: &Node, level: usize, writer: &mut impl Write) {
    //process node
    match node {
        Node::Heading(_) => {
            //track headers
            eprintln!("Node::Heading({level}) {}", extract_text(node));
        },
        Node::Paragraph(_) => {
            let text = extract_text(node);
            writeln!(writer, "{}", text).unwrap();
        },
        Node::Html(_) => {
            let text = extract_text(node);
            writeln!(writer, "{}", text).unwrap();
        },
        Node::Table(table) => {
            eprintln!("Node::Table");
            let text = extract_table(table);
            writeln!(writer, "{}", text).unwrap();
        },
        _ => {
            //process children
            if let Some(children) = node.children() {
                for child in children {
                    traverse_ast(child, level+1, writer);
                }
            }
        },
    };
}

fn extract_table(table: &markdown::mdast::Table) -> String {
    let mut output = String::new();
    for row in &table.children {
        output.push_str("| ");
        if let Some(cells) = row.children() {
            let text: Vec<String> = cells.iter()
                .map(extract_text)
                .collect();
            output.push_str(&text.join(" | "));
        }
        output.push_str(" |");
    }
    eprintln!("TABLE: {}",output);
    output
}

fn extract_text(node: &Node) -> String {
    match node {
        Node::Text(text) => text.value.clone(),
        Node::InlineCode(code) => code.value.clone(),
        Node::Html(html) => html.value.clone(),
        _ => {
            let mut result = String::new();
            if let Some(children) = node.children() {
                for child in children {
                    result.push_str(&extract_text(child));
                }
            }
            result
        }
    }
}

fn debug_print_tree(node: &Node, depth: usize) {
    eprintln!("{}{}", " ".repeat(depth), node_name(node));
    if let Some(children) = node.children() {
        for child in children {
            debug_print_tree(child, depth+1);
        }
    }
}
fn node_name(node: &Node) -> &'static str {
    match node {
        Node::Root(_) => "Root",
        Node::Blockquote(_) => "Blockquote",
        Node::FootnoteDefinition(_) => "FootnoteDefinition",
        Node::MdxJsxFlowElement(_) => "MdxJsxFlowElement",
        Node::List(_) => "List",
        Node::MdxjsEsm(_) => "MdxjsEsm",
        Node::Toml(_) => "Toml",
        Node::Yaml(_) => "Yaml",
        Node::Break(_) => "Break",
        Node::InlineCode(_) => "InlineCode",
        Node::InlineMath(_) => "InlineMath",
        Node::Delete(_) => "Delete",
        Node::Emphasis(_) => "Emphasis",
        Node::MdxTextExpression(_) => "MdxTextExpression",
        Node::FootnoteReference(_) => "FootnoteReference",
        Node::Html(_) => "Html",
        Node::Image(_) => "Image",
        Node::ImageReference(_) => "ImageReference",
        Node::MdxJsxTextElement(_) => "MdxJsxTextElement",
        Node::Link(_) => "Link",
        Node::LinkReference(_) => "LinkReference",
        Node::Strong(_) => "Strong",
        Node::Text(_) => "Text",
        Node::Code(_) => "Code",
        Node::Math(_) => "Math",
        Node::MdxFlowExpression(_) => "MdxFlowExpression",
        Node::Heading(_) => "Heading",
        Node::Table(_) => "Table",
        Node::ThematicBreak(_) => "ThematicBreak",
        Node::TableRow(_) => "TableRow",
        Node::TableCell(_) => "TableCell",
        Node::ListItem(_) => "ListItem",
        Node::Definition(_) => "Definition",
        Node::Paragraph(_) => "Paragraph",
    }
}
