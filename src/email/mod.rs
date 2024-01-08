use serde::{Deserialize, Serialize};

use crate::REQWEST_CLIENT;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailPayload {
    pub from: EmailContact,
    pub reply_to: Vec<EmailContact>,
    pub to: Vec<EmailContact>,
    pub cc: Vec<EmailContact>,
    pub bcc: Vec<EmailContact>,
    pub subject: String,
    pub content: Vec<EmailContent>,
}

#[derive(Serialize, Deserialize)]
pub struct EmailContact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub email: String,
}

#[derive(Serialize, Deserialize)]
pub struct EmailContent {
    pub mime: String,
    pub value: String,
}

pub async fn send_email_otp(to_email: &str, otp: &str) -> anyhow::Result<()> {
    let auth_tok = std::env::var("EMAIL_AUTH_TOK")?;
    let email_payload = EmailPayload {
        from: EmailContact {
            name: "Bill Divide".to_string().into(),
            email: "otp@billdivide.app".to_string(),
        },
        reply_to: vec![],
        cc: vec![],
        bcc: vec![],
        to: vec![EmailContact {
            name: None,
            email: to_email.to_string(),
        }],
        subject: "Your One-Time Passcode for Bill Divide Signup/Login".to_string(),
        content: vec![
            EmailContent{
                mime:"text/html".to_string(),
                value: r##"
<div style="font-family: Helvetica,Arial,sans-serif;min-width:1000px;overflow:auto;line-height:2">
  <div style="margin:50px auto;width:70%;padding:20px 0">
    <div style="border-bottom:1px solid #eee">
      <a href="" style="font-size:1.4em;color: #00466a;text-decoration:none;font-weight:600">Bill Divide</a>
    </div>
    <p style="font-size:1.1em">Hi,</p>
    <p>This is your One-Time Passcode for Bill Divide Signup/Login. Passcode is valid for 5 minutes</p>
    <h2 style="background: #00466a;margin: 0 auto;width: max-content;padding: 0 10px;color: #fff;border-radius: 4px;">{{{PASSCODE}}}</h2>
    <p style="font-size:0.9em;">Regards,<br />Bill Divide</p>
    <hr style="border:none;border-top:1px solid #eee" />
  </div>
</div>
                "##.replace("{{{PASSCODE}}}", otp)
            }
        ],
    };
    let request = REQWEST_CLIENT
        .post("https://worker-email-production.deepgauravraj.workers.dev/api/email")
        .header("Authorization", auth_tok)
        .json(&email_payload)
        .send()
        .await?;
    if !request.status().is_success() {
        Err(anyhow::anyhow!("Cant send email"))
    } else {
        Ok(())
    }
}

pub async fn send_email_invite(to_email: &str, inviter: &str) -> anyhow::Result<()> {
    let auth_tok = std::env::var("EMAIL_AUTH_TOK")?;
    let email_payload = EmailPayload {
        from: EmailContact {
            name: "Bill Divide".to_string().into(),
            email: "invite@billdivide.app".to_string(),
        },
        reply_to: vec![],
        cc: vec![],
        bcc: vec![],
        to: vec![EmailContact {
            name: None,
            email: to_email.to_string(),
        }],
        subject: "Join [INVITER_NAME] on Bill Divide for Easy Expense Sharing".replace("[INVITER_NAME]", inviter),
        content: vec![
            EmailContent{
                mime:"text/html".to_string(),
                value: r##"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta http-equiv="Content-Type" content="text/html charset=UTF-8" />
    <title>Join Bill Divide - Expense Sharing Made Easy</title>
</head>
<body style="font-family: Arial, sans-serif; line-height: 1.6; color: #333; margin: 0; padding: 0;">
    <table role="presentation" width="100%" border="0" cellspacing="0" cellpadding="0">
    <tr>
        <td style="padding: 20px;">
        <h2 style="font-size: 24px;">Join Bill Divide - Expense Sharing Made Easy!</h2>
        <p>Hello there!</p>
        <p>You've been invited by <strong>[INVITER_NAME]</strong> to join Bill Divide, a convenient app to share expenses seamlessly with friends. <strong>[INVITER_NAME]</strong> wants to share an expense with you!</p>
        <p style="margin-bottom: 20px;">To start splitting bills hassle-free, simply <a href="https://billdivide.app/" style="display: inline-block; padding: 10px 20px; text-decoration: none; background-color: #007bff; color: #fff; border-radius: 3px;">Join Now</a></p>
        <p>If the button above doesn't work, you can copy and paste the following link into your browser:</p>
        <p style="margin-bottom: 20px;">https://billdivide.app/</p>
        <p>We're excited to have you on board!</p>
        <p>Best regards,<br>Your Bill Divide Team</p>
        </td>
    </tr>
    </table>
</body>
</html>        
                "##.replace("[INVITER_NAME]", inviter)
            }
        ],
    };
    let request = REQWEST_CLIENT
        .post("https://worker-email-production.deepgauravraj.workers.dev/api/email")
        .header("Authorization", auth_tok)
        .json(&email_payload)
        .send()
        .await?;
    if !request.status().is_success() {
        Err(anyhow::anyhow!("Cant send email"))
    } else {
        Ok(())
    }
}
