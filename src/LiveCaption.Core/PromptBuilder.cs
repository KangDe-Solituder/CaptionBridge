namespace LiveCaption.Core;

public static class PromptBuilder
{
    public static (string System, string User) Build(TranslationRequest request)
    {
        return request.Mode switch
        {
            TranslationMode.Explanation => (
                "你是简洁、可靠的语言助手。使用中文解释用户给出的单词或句子；说明自然含义、必要时的读音和关键表达。不要臆造上下文，不要使用 Markdown 标题。",
                request.SourceText),
            TranslationMode.LiveCaption => (
                $"你是实时字幕翻译器。将输入内容翻译成{request.TargetLanguage}，只输出译文。保留人名、游戏名、口语语气和不完整句，不要解释，不要补写原文没有的信息。",
                request.SourceText),
            _ => (
                $"你是翻译器。自动识别源语言，将文本翻译成{request.TargetLanguage}。只输出自然、简洁的译文，不要解释。",
                request.SourceText)
        };
    }
}
