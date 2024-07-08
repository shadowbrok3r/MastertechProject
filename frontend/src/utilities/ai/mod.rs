use async_openai_wasm::{
    config::OpenAIConfig, types::{
        ChatCompletionFunctionsArgs, ChatCompletionRequestFunctionMessageArgs, ChatCompletionRequestUserMessageArgs, ChatCompletionToolArgs, ChatCompletionToolType, CreateChatCompletionRequestArgs, FunctionObjectArgs
    }, Client
};
use serde_json::json;
use std::collections::HashMap;
use std::error::Error;

const OPENAI_API_KEY: &str = "sk-proj-2iAUAr8GOsVMObnXUXXFT3BlbkFJismARv2v881R7GnnqNU9";
async fn tests() -> Result<(), Box<dyn Error>> {

    let config = OpenAIConfig::new().with_api_key(OPENAI_API_KEY);
    let client = Client::with_config(config);

    let user_prompt = "What's the weather like in Boston and Atlanta?";

    let request = CreateChatCompletionRequestArgs::default()
        .max_tokens(512u16)
        .model("gpt-4-1106-preview")
        .messages([
            ChatCompletionRequestUserMessageArgs::default()
                .content(user_prompt)
                .build()?
                .into()
        ])
        .tools(vec![ChatCompletionToolArgs::default()
            .r#type(ChatCompletionToolType::Function)
            .function(
                FunctionObjectArgs::default()
                    .name("get_current_weather")
                    .description("Get the current weather in a given location")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city and state, e.g. San Francisco, CA",
                            },
                            "unit": { "type": "string", "enum": ["celsius", "fahrenheit"] },
                        },
                        "required": ["location"],
                    }))
                    .build()?,
            )
            .build()?])
        .build()?;

    let response_message = client
        .chat()
        .create(request)
        .await?
        .choices
        .get(0)
        .unwrap()
        .message
        .clone();

    if let Some(function_call) = response_message.tool_calls {
        for tool_call in function_call {
            let mut available_functions: HashMap<&str, fn(&str, &str) -> serde_json::Value> =
                HashMap::new();
            available_functions.insert("get_current_weather", get_current_weather);
            let function_name = tool_call.function.name;
            let function_args: serde_json::Value = tool_call.function.arguments.parse().unwrap();

            let location = function_args["location"].as_str().unwrap();
            let unit = "fahrenheit";
            let function = available_functions.get(function_name.as_str()).unwrap();
            let function_response = function(location, unit);

            let message = vec![
                ChatCompletionRequestUserMessageArgs::default()
                    .content("What's the weather like in Boston?")
                    .build()?
                    .into(),
                ChatCompletionRequestFunctionMessageArgs::default()
                    .content(function_response.to_string())
                    .name(function_name)
                    .build()?
                    .into(),
            ];

            println!("{}", serde_json::to_string(&message).unwrap());

            let request = CreateChatCompletionRequestArgs::default()
                .max_tokens(512u16)
                .model("gpt-3.5-turbo-0613")
                .messages(message)
                .build()?;

            let response = client.chat().create(request).await?;

            println!("\nResponse:\n");
            for choice in response.choices {
                println!(
                    "{}: Role: {}  Content: {:?}",
                    choice.index, choice.message.role, choice.message.content
                );
            }
        }
    }

    Ok(())
}

fn get_current_weather(location: &str, unit: &str) -> serde_json::Value {
    let weather_info = json!({
        "location": location,
        "temperature": "72",
        "unit": unit,
        "forecast": ["sunny", "windy"]
    });

    weather_info
}