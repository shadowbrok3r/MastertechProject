use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use reqwest::Client;
use serde::Serialize;
use crate::data::{TicketInformation, SystemInformation};

use super::scaffold::{Salesman, Techs};


// #[derive(Serialize)]
pub struct AsanaTask{
    pub task_name: String,
    pub html_notes: String,
    pub assignee: TaskAssignee,
    pub file_attachment: Option<PathBuf>
}

// #[derive(Serialize)]
pub struct TaskAssignee{
    pub salesman: Salesman,
    pub tech: Techs
}

/*
pub fn html_builder(
    ticket_info: TicketInformation, 
    system_info: SystemInformation,
    client: Client,
    scaffold_request: Sender<String>
){
    ;

    if store.as_str() == "RIV"{

    }
    else{
        let mtech_username = dotenv::var("MTECH_EMAIL").unwrap_or("not provided".to_string());
        let mtech_password = dotenv::var("MTECH_PASS").unwrap_or("not provided".to_string());

        let email_content = email_builder();

        let store_email = &ticket_info.jurisdiction.store_email();

        let email = Message::builder()
            .from("TUR SHEET <pcl.mastertech@gmail.com>".parse().unwrap())
            .to("logan.lees@pclaptops.com".parse().unwrap())
            .subject(format!("{cust} - {so_num}"))
            .header(ContentType::TEXT_HTML)
            .body(email_content)
            .unwrap();

        let creds = Credentials::new(mtech_username.to_owned(), mtech_password.to_owned());

        // Open a remote connection to gmail
        let mailer = SmtpTransport::relay("smtp.gmail.com")
            .unwrap()
            .credentials(creds)
            .build();

        output_text += "\n {store_email} {email}";

        // Send the email
        match mailer.send(&email) {
            Ok(_) => println!("Email sent successfully!"),
            Err(e) => {
                output_text += "\n{e:?}";
                println!("Could not send email: {e:?}")
            },
        }
    }
        
}

pub fn asana_html_builder(
    ticket_info: TicketInformation, 
    system_info: SystemInformation,
    send_specs: bool,
    client: Client,
    scaffold_request: Sender<String>
) -> AsanaTask { 
    let mut salesman_map = HashMap::new();
    let mut tech_map = HashMap::new();

    let salesman = &format!("{}", &salesman_cbox);
    let checkin_rep = &ticket_info.user_id;
    let technician = &format!("{}", &techs_cbox);

    salesman_map.insert("Jake", "1202792432658520");
    salesman_map.insert("Danny", "1202791016369879");
    tech_map.insert("Logan", "1199992640930465");
    tech_map.insert("Bread", "1202792432421640");
    tech_map.insert("Taco", "1202792432551073");

    let hdd_test = &format!("{:?}", &hdd_test_cbox);
    let ram_test = &format!("{:?}", &ram_test_cbox);
    let ssd_test = &format!("{:?}", &ssd_test_cbox);

    let checkin_notes = &ticket_info.checkin_notes;
    let recommendations = &recommendations;   
    let task_name = (cust, so_num);
    let assignees = (salesman, technician);

    let mut attached_file: Option<PathBuf> = None;
    
    if let Some(file) = &opened_file{
        attached_file = Some(file.to_path_buf());
    }

    let mut specs = String::new();
    let cps = antivirus_installed.clone();
    let mut final_disk = String::new();
    let mut each_disk = String::new();
                                            
    let cust_code = &ticket_info.cust_code;
    let doc_alias = &ticket_info.doc_alias;
    //let department = &ticket_info.department;
    //let juris = &ticket_info.jurisdiction;
    let inv_amt = &ticket_info.invoice_amnt;
    let cust_email = &ticket_info.customer_email;
    let last_inv_num = &ticket_info.last_invoice_number;
    let last_inv_amt = &ticket_info.last_invoice_amount;
    let total_inv_num = &ticket_info.total_invoice_count;
    let phone1 = &ticket_info.customer_phone_1;
    let phone2 = &ticket_info.customer_phone_2;
    let mut phone_2 = String::new();
    if !phone2.is_empty(){
        phone_2 = format!
        ("
            <tr>
                <td style=\"padding:1px 1px\">Phone #2</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{phone2}</td>
            </tr>
        ");
    }

    let extra_customer_info = format!
    ("
        <tr>
            <td style=\"padding:1px 1px\">Customer Code</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_code}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Phone #</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\"><strong>{phone1}</strong></td>
        </tr>
        {phone_2}
        <tr>
            <td style=\"padding:1px 1px\">Customer Email</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_email}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Current Total</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">${inv_amt}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Last SI#</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_num}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Last Invoice Total</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_amt}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\"># of SI's</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{total_inv_num}</td>
        </tr>
    ");
    if send_specs == true{
        let system_name = &system_info.system_name;
        let cpu_name = &system_info.cpu_name;
        let total_ram = &system_info.total_ram;
        let gpu = &system_info.gpu.clone().unwrap_or("no gpu detected".to_string());

        for index in 0..disk_num
        {
            if let Some(disk) = disks.get(index)
            {
                let disk_letter = format!("{}", disk.get("letter").and_then(Value::as_str).unwrap_or(""));
                let disk_available = format!("{} Gb", disk.get("available space").and_then(Value::as_str).unwrap_or(""));
                let disk_total = format!("{} Gb", disk.get("total space").and_then(Value::as_str).unwrap_or(""));

                each_disk += &format!
                ("
                    <tr>
                        <td style=\"padding:1px 1px\">        {disk_letter}</td>
                        <td style=\"padding:1px 1px\">        {disk_available}</td>
                        <td style=\"padding:1px 1px\">        {disk_total}</td>
                    </tr>
                ");

                final_disk = format!
                ("
                    <tr>
                        <td style=\"padding:1px 4px\">Letter</td>
                        <td style=\"padding:1px 4px\">Avail Space</td>
                        <td style=\"padding:1px 4px\">Total Space</td>
                    </tr>
                    {each_disk}
                ");

            }
        }

        specs = format!("
            <table>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,200,200\" width=\"450\"
                    >              <code>       Computer Info        </code></td>
                </tr>
                <tr>
                    <td>OS</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{system_name}</td>
                </tr>
                <tr>
                    <td>CPU</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{cpu_name}</td>
                </tr>
                <tr>
                    <td>RAM</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{total_ram} Gb</td>
                </tr>
                <tr>
                    <td>GPU</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{gpu}</td>
                </tr>
                <tr>
                    <td>Antivirus</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{cps}</td>
                </tr>
                <tr>
                    <td>SEB</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\"></td>
                </tr>
                <tr>
                <td colspan=
                \"3\" data-cell-widths=\"100,200,200\" width=\"400\" style=\"text-align:center;\"
                >                <code>        HDD/SSD info        </code></td>
                </tr>
                {final_disk}
            </table>
        ").trim().to_string();

    }else{
        specs = "Computer information was not sent with ticket".to_string();
    }
    
    let html_notes = format!
    ("
        <body>
            <table>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                    >                <code>        {doc_alias} Info        </code>
                    </td>
                </tr>
                <tr>
                    <td style=\"padding:1px 1px\">Salesman</td>
                    <td style=\"padding:1px 1px\">Checkin Rep</td>
                    <td style=\"padding:1px 1px\">Technician</td>
                </tr>
                <tr>
                    <td style=\"padding:1px 4px\">     {salesman}</td>
                    <td style=\"padding:1px 4px\">     {checkin_rep}</td>
                    <td style=\"padding:1px 4px\">     {technician}</td>
                </tr>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                    >                <code>           Customer           </code>
                    </td>
                </tr>
                {extra_customer_info}
            </table>
            {specs}
            <ul>
                <li><strong>SSD test:</strong>     {ssd_test}</li>
                <li><strong>HDD test:</strong>     {hdd_test}</li>
                <li><strong>RAM test:</strong>     {ram_test}</li>
            </ul>
            <h2><strong><code>           Notes           </code></strong></h2>
            <ul><li><strong>        Checkin Notes:      </strong>     \n{checkin_notes}</li>
                <li><strong>        Recommendations:        </strong>     \n{recommendations}</li></ul>
        </body>"
    );

    AsanaTask { 
        task_name: format!("{cust} - {so_num}"), 
        html_notes: html_notes, 
        assignee: TaskAssignee { 
            salesman: (), 
            tech: () 
        }, 
        file_attachment: () 
    }

}


*/
 pub fn email_builder<T >() -> String {

     let html_string = format!("
 <!doctype html>
<html ⚡4email data-css-strict>
 <head><meta charset=\"utf-8\"><style amp4email-boilerplate>body{{visibility:hidden}}</style><script async src=\"https://cdn.ampproject.org/v0.js\"></script>
  
  <style amp-custom>
.es-desk-hidden {{
	display:none;
	float:left;
	overflow:hidden;
	width:0;
	max-height:0;
	line-height:0;
}}
body {{
	width:100%;
	font-family:\"open sans\", \"helvetica neue\", helvetica, arial, sans-serif;
}}
table {{
	border-collapse:collapse;
	border-spacing:0px;
}}
table td, body, .es-wrapper {{
	padding:0;
	Margin:0;
}}
.es-content, .es-header, .es-footer {{
	table-layout:fixed;
	width:100%;
}}
p, hr {{
	Margin:0;
}}
h1, h2, h3, h4, h5 {{
	Margin:0;
	line-height:100%;
	font-family:\"trebuchet ms\", \"lucida grande\", \"lucida sans unicode\", \"lucida sans\", tahoma, sans-serif;
}}
.es-left {{
	float:left;
}}
.es-right {{
	float:right;
}}
.es-p5 {{
	padding:5px;
}}
.es-p5t {{
	padding-top:5px;
}}
.es-p5b {{
	padding-bottom:5px;
}}
.es-p5l {{
	padding-left:5px;
}}
.es-p5r {{
	padding-right:5px;
}}
.es-p10 {{
	padding:10px;
}}
.es-p10t {{
	padding-top:10px;
}}
.es-p10b {{
	padding-bottom:10px;
}}
.es-p10l {{
	padding-left:10px;
}}
.es-p10r {{
	padding-right:10px;
}}
.es-p15 {{
	padding:15px;
}}
.es-p15t {{
	padding-top:15px;
}}
.es-p15b {{
	padding-bottom:15px;
}}
.es-p15l {{
	padding-left:15px;
}}
.es-p15r {{
	padding-right:15px;
}}
.es-p20 {{
	padding:20px;
}}
.es-p20t {{
	padding-top:20px;
}}
.es-p20b {{
	padding-bottom:20px;
}}
.es-p20l {{
	padding-left:20px;
}}
.es-p20r {{
	padding-right:20px;
}}
.es-p25 {{
	padding:25px;
}}
.es-p25t {{
	padding-top:25px;
}}
.es-p25b {{
	padding-bottom:25px;
}}
.es-p25l {{
	padding-left:25px;
}}
.es-p25r {{
	padding-right:25px;
}}
.es-p30 {{
	padding:30px;
}}
.es-p30t {{
	padding-top:30px;
}}
.es-p30b {{
	padding-bottom:30px;
}}
.es-p30l {{
	padding-left:30px;
}}
.es-p30r {{
	padding-right:30px;
}}
.es-p35 {{
	padding:35px;
}}
.es-p35t {{
	padding-top:35px;
}}
.es-p35b {{
	padding-bottom:35px;
}}
.es-p35l {{
	padding-left:35px;
}}
.es-p35r {{
	padding-right:35px;
}}
.es-p40 {{
	padding:40px;
}}
.es-p40t {{
	padding-top:40px;
}}
.es-p40b {{
	padding-bottom:40px;
}}
.es-p40l {{
	padding-left:40px;
}}
.es-p40r {{
	padding-right:40px;
}}
.es-menu td {{
	border:0;
}}
s {{
	text-decoration:line-through;
}}
p, ul li, ol li {{
	font-family:\"open sans\", \"helvetica neue\", helvetica, arial, sans-serif;
	line-height:120%;
	margin-bottom:11px;
}}
ul li, ol li {{
	margin-left:0;
}}
a {{
	text-decoration:underline;
}}
.es-menu td a {{
	text-decoration:none;
	display:block;
	font-family:\"open sans\", \"helvetica neue\", helvetica, arial, sans-serif;
}}
.es-wrapper {{
	width:100%;
	height:100%;
}}
.es-wrapper-color, .es-wrapper {{
	background-color:#131313;
}}
.es-header {{
	background-color:transparent;
}}
.es-header-body {{
	background-color:transparent;
}}
.es-header-body p, .es-header-body ul li, .es-header-body ol li {{
	color:#FFFFFF;
	font-size:12px;
	margin-bottom:8px;
}}
.es-header-body a {{
	color:#FFFFFF;
	font-size:12px;
}}
.es-content-body {{
	background-color:#FFFFFF;
}}
.es-content-body p, .es-content-body ul li, .es-content-body ol li {{
	color:#081D36;
	font-size:18px;
	margin-bottom:11px;
}}
.es-content-body a {{
	color:#081D36;
	font-size:18px;
}}
.es-footer {{
	background-color:transparent;
}}
.es-footer-body {{
	background-color:#FFFFFF;
}}
.es-footer-body p, .es-footer-body ul li, .es-footer-body ol li {{
	color:#081D36;
	font-size:14px;
	margin-bottom:9px;
}}
.es-footer-body a {{
	color:#081D36;
	font-size:14px;
}}
.es-infoblock, .es-infoblock p, .es-infoblock ul li, .es-infoblock ol li {{
	line-height:120%;
	font-size:12px;
	color:#CCCCCC;
	margin-bottom:8px;
}}
.es-infoblock a {{
	font-size:12px;
	color:#CCCCCC;
}}
h1 {{
	font-size:40px;
	font-style:normal;
	font-weight:bold;
	color:#081D36;
	margin-bottom:20px;
}}
h2 {{
	font-size:24px;
	font-style:normal;
	font-weight:bold;
	color:#081D36;
	margin-bottom:12px;
}}
h3 {{
	font-size:20px;
	font-style:normal;
	font-weight:bold;
	color:#081D36;
	margin-bottom:10px;
}}
.es-header-body h1 a, .es-content-body h1 a, .es-footer-body h1 a {{
	font-size:40px;
}}
.es-header-body h2 a, .es-content-body h2 a, .es-footer-body h2 a {{
	font-size:24px;
}}
.es-header-body h3 a, .es-content-body h3 a, .es-footer-body h3 a {{
	font-size:20px;
}}
a.es-button, button.es-button {{
	padding:10px 30px 10px 30px;
	display:inline-block;
	background:#B2222D;
	border-radius:0px;
	font-size:18px;
	font-family:arial, \"helvetica neue\", helvetica, sans-serif;
	font-weight:normal;
	font-style:normal;
	line-height:120%;
	color:#FFFFFF;
	text-decoration:none;
	width:auto;
	text-align:center;
}}
.es-button-border {{
	border-style:solid solid solid solid;
	border-color:#2CB543 #2CB543 #2CB543 #2CB543;
	background:#B2222D;
	border-width:0px 0px 0px 0px;
	display:inline-block;
	border-radius:0px;
	width:auto;
}}
.es-menu amp-img, .es-button amp-img {{
	vertical-align:middle;
}}
@media only screen and (max-width:600px) {{p, ul li, ol li, a {{ line-height:120% }} h1, h2, h3, h1 a, h2 a, h3 a {{ line-height:100% }} h1 {{ font-size:30px; text-align:center; margin-bottom:15px }} h2 {{ font-size:24px; text-align:center; margin-bottom:12px }} h3 {{ font-size:20px; text-align:center; margin-bottom:10px }} .es-header-body h1 a, .es-content-body h1 a, .es-footer-body h1 a {{ font-size:30px; text-align:center }} .es-header-body h2 a, .es-content-body h2 a, .es-footer-body h2 a {{ font-size:24px; text-align:center }} .es-header-body h3 a, .es-content-body h3 a, .es-footer-body h3 a {{ font-size:20px; text-align:center }} .es-menu td a {{ font-size:12px }} .es-header-body p, .es-header-body ul li, .es-header-body ol li, .es-header-body a {{ font-size:14px }} .es-content-body p, .es-content-body ul li, .es-content-body ol li, .es-content-body a {{ font-size:14px }} .es-footer-body p, .es-footer-body ul li, .es-footer-body ol li, .es-footer-body a {{ font-size:12px }} .es-infoblock p, .es-infoblock ul li, .es-infoblock ol li, .es-infoblock a {{ font-size:12px }} *[class=\"gmail-fix\"] {{ display:none }} .es-m-txt-c, .es-m-txt-c h1, .es-m-txt-c h2, .es-m-txt-c h3 {{ text-align:center }} .es-m-txt-r, .es-m-txt-r h1, .es-m-txt-r h2, .es-m-txt-r h3 {{ text-align:right }} .es-m-txt-l, .es-m-txt-l h1, .es-m-txt-l h2, .es-m-txt-l h3 {{ text-align:left }} .es-m-txt-r amp-img {{ float:right }} .es-m-txt-c amp-img {{ margin:0 auto }} .es-m-txt-l amp-img {{ float:left }} .es-button-border {{ display:inline-block }} a.es-button, button.es-button {{ font-size:18px; display:inline-block }} .es-adaptive table, .es-left, .es-right {{ width:100% }} .es-content table, .es-header table, .es-footer table, .es-content, .es-footer, .es-header {{ width:100%; max-width:600px }} .es-adapt-td {{ display:block; width:100% }} .adapt-img {{ width:100%; height:auto }} td.es-m-p0 {{ padding:0 }} td.es-m-p0r {{ padding-right:0 }} td.es-m-p0l {{ padding-left:0 }} td.es-m-p0t {{ padding-top:0 }} td.es-m-p0b {{ padding-bottom:0 }} td.es-m-p20b {{ padding-bottom:20px }} .es-mobile-hidden, .es-hidden {{ display:none }} tr.es-desk-hidden, td.es-desk-hidden, table.es-desk-hidden {{ width:auto; overflow:visible; float:none; max-height:inherit; line-height:inherit }} tr.es-desk-hidden {{ display:table-row }} table.es-desk-hidden {{ display:table }} td.es-desk-menu-hidden {{ display:table-cell }} .es-menu td {{ width:1% }} table.es-table-not-adapt, .esd-block-html table {{ width:auto }} table.es-social {{ display:inline-block }} table.es-social td {{ display:inline-block }} .es-desk-hidden {{ display:table-row; width:auto; overflow:visible; max-height:inherit }} td.es-m-p5 {{ padding:5px }} td.es-m-p5t {{ padding-top:5px }} td.es-m-p5b {{ padding-bottom:5px }} td.es-m-p5r {{ padding-right:5px }} td.es-m-p5l {{ padding-left:5px }} td.es-m-p10 {{ padding:10px }} td.es-m-p10t {{ padding-top:10px }} td.es-m-p10b {{ padding-bottom:10px }} td.es-m-p10r {{ padding-right:10px }} td.es-m-p10l {{ padding-left:10px }} td.es-m-p15 {{ padding:15px }} td.es-m-p15t {{ padding-top:15px }} td.es-m-p15b {{ padding-bottom:15px }} td.es-m-p15r {{ padding-right:15px }} td.es-m-p15l {{ padding-left:15px }} td.es-m-p20 {{ padding:20px }} td.es-m-p20t {{ padding-top:20px }} td.es-m-p20r {{ padding-right:20px }} td.es-m-p20l {{ padding-left:20px }} td.es-m-p25 {{ padding:25px }} td.es-m-p25t {{ padding-top:25px }} td.es-m-p25b {{ padding-bottom:25px }} td.es-m-p25r {{ padding-right:25px }} td.es-m-p25l {{ padding-left:25px }} td.es-m-p30 {{ padding:30px }} td.es-m-p30t {{ padding-top:30px }} td.es-m-p30b {{ padding-bottom:30px }} td.es-m-p30r {{ padding-right:30px }} td.es-m-p30l {{ padding-left:30px }} td.es-m-p35 {{ padding:35px }} td.es-m-p35t {{ padding-top:35px }} td.es-m-p35b {{ padding-bottom:35px }} td.es-m-p35r {{ padding-right:35px }} td.es-m-p35l {{ padding-left:35px }} td.es-m-p40 {{ padding:40px }} td.es-m-p40t {{ padding-top:40px }} td.es-m-p40b {{ padding-bottom:40px }} td.es-m-p40r {{ padding-right:40px }} td.es-m-p40l {{ padding-left:40px }} p, ul li, ol li {{ margin-bottom:11px }} .es-header-body p, .es-header-body ul li, .es-header-body ol li {{ margin-bottom:9px }} .es-footer-body p, .es-footer-body ul li, .es-footer-body ol li {{ margin-bottom:8px }} .es-infoblock p, .es-infoblock ul li, .es-infoblock ol li {{ margin-bottom:8px }} }}
</style>
 </head>
 <body>
  <div dir=\"ltr\" class=\"es-wrapper-color\" lang=\"en\">
   <!--[if gte mso 9]>
			<v:background xmlns:v=\"urn:schemas-microsoft-com:vml\" fill=\"t\">
				<v:fill type=\"tile\" color=\"#131313\" origin=\"0.5, 0\" position=\"0.5, 0\"></v:fill>
			</v:background>
		<![endif]-->
   <table class=\"es-wrapper\" style=\"background-position: center top\">
     <tr>
      <td valign=\"top\">
       <table class=\"es-header\" align=\"center\">
         <tr>
          <td align=\"center\">
           <table bgcolor=\"#ffffff\" class=\"es-header-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" width=\"700\">
             <tr>
              <td class=\"es-p20\" align=\"left\">
               <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                 <tr>
                  <td width=\"660\" class=\"es-m-p0r\" valign=\"top\" align=\"center\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"center\" class=\"es-m-txt-c\" style=\"font-size: 0px\"><a target=\"_blank\" href=\"https://viewstripo.email\"><amp-img src=\"https://ecwnuzs.stripocdn.email/content/guids/89070ba8-83e1-4d67-b646-4663532227d2/images/masterlogov2_1.png\" alt=\"Logo\" style=\"display: block\" title=\"Logo\" height=\"70\" width=\"70\"></amp-img></a></td>
                     </tr>
                   </table></td>
                 </tr>
               </table></td>
             </tr>
           </table></td>
         </tr>
       </table>
       <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-content\" align=\"center\">
         <tr>
          <td align=\"center\">
           <table bgcolor=\"#ffffff\" class=\"es-content-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" width=\"700\" style=\"border-radius: 50px 50px 0 0\">
             <tr>
              <td align=\"left\">
               <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                 <tr>
                  <td width=\"700\" align=\"center\" valign=\"top\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"border-radius: 4px;border-collapse: separate\">
                     <tr>
                      <td align=\"center\" class=\"es-p5t es-m-p20r es-m-p20l\"><h2 style=\"color: #000000;line-height: 200%\"><b>CUST_NAME - SO_NUMBER</b></h2></td>
                     </tr>
                   </table></td>
                 </tr>
               </table></td>
             </tr>
             <tr>
              <td class=\"es-p40t es-p20r es-p20l\" align=\"left\" bgcolor=\"#22272f\" style=\"background-color: #22272f\">
               <!--[if mso]><table width=\"660\" cellpadding=\"0\" cellspacing=\"0\"><tr><td width=\"315\" valign=\"top\"><![endif]-->
               <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-left\" align=\"left\">
                 <tr>
                  <td width=\"315\" class=\"es-m-p20b\" align=\"left\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"center\"><h2 style=\"text-align: center;color: #fcfdfd;font-family: &quot;times new roman&quot;, times, baskerville, georgia, serif\"><strong>Customer Info</strong></h2>
                       <table border=\"2\" align=\"center\" cellspacing=\"2\" cellpadding=\"2\" class=\"es-table\" style=\"width: 100%\">
                         <tr>
                          <td style=\"color: #ffffff\"><strong>Phone #</strong></td>
                          <td style=\"text-align: center;color: #ffffff\">phone_number_1</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>Phone # 2</strong></td>
                          <td style=\"text-align: center;color: #ffffff\">phone_number_2</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>Email</strong></td>
                          <td style=\"color: #ffffff;text-align: center\">your_email@google.com</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>Ticket Total</strong></td>
                          <td><p style=\"color: #fcfdfd\"><br></p></td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>Last SI #</strong></td>
                          <td><p style=\"color: #fcfdfd\"><br></p></td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>Last SI $</strong></td>
                          <td><p style=\"color: #fcfdfd\"><br></p></td>
                         </tr>
                       </table><p style=\"color: #fcfdfd\"><br></p></td>
                     </tr>
                   </table></td>
                 </tr>
               </table> 
               <!--[if mso]></td><td width=\"30\"></td><td width=\"315\" valign=\"top\"><![endif]-->
               <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-right\" align=\"right\">
                 <tr>
                  <td width=\"315\" align=\"left\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"left\"><h2 style=\"text-align: center;color: #ffffff;font-family: 'times new roman', times, baskerville, georgia, serif\"><strong>Hardware Info</strong></h2>
                       <table border=\"2\" align=\"center\" cellspacing=\"2\" cellpadding=\"2\" class=\"es-table\" style=\"width: 100%\">
                         <tr>
                          <td style=\"color: #ffffff\"><strong>CPU</strong></td>
                          <td style=\"text-align: center;color: #ffffff\">Ryzen 9 5950X</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>GPU</strong></td>
                          <td style=\"text-align: center;color: #ffffff\">RTX 3090</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><strong>RAM</strong></td>
                          <td style=\"text-align: center;color: #ffffff\">32 Gb</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><b>OS</b></td>
                          <td><p style=\"text-align: center;color: #ffffff\">Windows 11</p></td>
                         </tr>
                       </table><p><br></p></td>
                     </tr>
                   </table></td>
                 </tr>
                 <tr>
                  <td width=\"315\" align=\"left\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"left\"><h3 style=\"text-align: center;color: #ffffff;font-family: 'times new roman', times, baskerville, georgia, serif\"><strong>Drives</strong></h3>
                       <table border=\"2\" align=\"center\" cellspacing=\"2\" cellpadding=\"2\" class=\"es-table\" style=\"width: 100%\">
                         <tr>
                          <td style=\"color: #ffffff\">C:\</td>
                          <td style=\"text-align: center;color: #ffffff\">325 / 1024 Gb</td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><br></td>
                          <td style=\"text-align: center;color: #ffffff\"><br></td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><br></td>
                          <td style=\"text-align: center;color: #ffffff\"><br></td>
                         </tr>
                         <tr>
                          <td style=\"color: #ffffff\"><br></td>
                          <td><p style=\"text-align: center;color: #ffffff\"><br></p></td>
                         </tr>
                       </table><p><br></p></td>
                     </tr>
                   </table></td>
                 </tr>
               </table> 
               <!--[if mso]></td></tr></table><![endif]--></td>
             </tr>
           </table></td>
         </tr>
       </table>
       <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-content\" align=\"center\">
         <tr>
          <td align=\"center\">
           <table bgcolor=\"#ffffff\" class=\"es-content-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" width=\"700\">
             <tr>
              <td align=\"left\" bgcolor=\"#2d3644\" style=\"background-color: #2d3644\">
               <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                 <tr>
                  <td width=\"700\" align=\"left\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"center\" class=\"es-p10\" style=\"font-size:0\">
                       <table border=\"0\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\">
                         <tr>
                          <td style=\"border-bottom: 1px solid #11feda;background: unset;height: 1px;width: 100%;margin: 0px\"></td>
                         </tr>
                       </table></td>
                     </tr>
                     <tr>
                      <td align=\"center\"><h2 style=\"color: #ffffff;line-height: 150%\">Service Notes</h2></td>
                     </tr>
                     <tr>
                      <td align=\"center\" class=\"es-p10\" style=\"font-size:0\">
                       <table border=\"0\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\">
                         <tr>
                          <td style=\"border-bottom: 1px solid #11feda;background: unset;height: 1px;width: 100%;margin: 0px\"></td>
                         </tr>
                       </table></td>
                     </tr>
                   </table></td>
                 </tr>
               </table></td>
             </tr>
             <tr>
              <td class=\"es-p40t es-p25b es-p20r es-p20l\" align=\"left\" bgcolor=\"#22272f\" style=\"background-color: #22272f\">
               <!--[if mso]><table width=\"660\" cellpadding=\"0\" cellspacing=\"0\"><tr><td width=\"320\" valign=\"top\"><![endif]-->
               <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-left\" align=\"left\">
                 <tr>
                  <td width=\"320\" class=\"es-m-p20b\" align=\"left\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"center\"><h2 style=\"color: #ffffff;line-height: 150%\">Checkin Notes</h2></td>
                     </tr>
                     <tr>
                      <td align=\"center\"><p style=\"color: #ffffff;line-height: 150%\">These are the checkin notes</p></td>
                     </tr>
                   </table></td>
                 </tr>
               </table> 
               <!--[if mso]></td><td width=\"20\"></td><td width=\"319\" valign=\"top\"><![endif]-->
               <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-right\" align=\"right\">
                 <tr>
                  <td width=\"319\" align=\"left\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"center\"><h2 style=\"color: #ffffff;line-height: 150%\">Recommendations</h2></td>
                     </tr>
                     <tr>
                      <td align=\"center\"><p style=\"color: #ffffff;line-height: 150%\">These are the recommendations</p></td>
                     </tr>
                   </table></td>
                 </tr>
               </table> 
               <!--[if mso]></td></tr></table><![endif]--></td>
             </tr>
           </table></td>
         </tr>
       </table>
       <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-footer\" align=\"center\">
         <tr>
          <td align=\"center\">
           <table bgcolor=\"#ffffff\" class=\"es-footer-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" width=\"700\" style=\"border-radius: 0 0 50px 50px\">
             <tr>
              <td class=\"es-p5\" align=\"left\">
               <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                 <tr>
                  <td width=\"690\" align=\"center\" valign=\"top\">
                   <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\">
                     <tr>
                      <td align=\"center\" class=\"made_with\" style=\"font-size: 0px\"><a target=\"_blank\" href=\"https://viewstripo.email/?utm_source=templates&utm_medium=email&utm_campaign=gadget_11&utm_content=santa_claus_brought_gifts\"><amp-img src=\"https://ecwnuzs.stripocdn.email/content/guids/89070ba8-83e1-4d67-b646-4663532227d2/images/pcllogo.png\" alt width=\"60\" style=\"display: block\" height=\"60\"></amp-img></a></td>
                     </tr>
                   </table></td>
                 </tr>
               </table></td>
             </tr>
           </table></td>
         </tr>
       </table></td>
     </tr>
   </table>
  </div>
 </body>
</html>");
     html_string
 }