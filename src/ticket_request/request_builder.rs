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

// pub fn email_builder() -> String {  }